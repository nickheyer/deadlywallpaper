//! Wallpaper Engine display conditions: JavaScript-style boolean expressions over sibling
//! property values, such as `showclock.value == true && style.value != "plain"`.

use serde_json::Value;

/// Evaluate `expr` with `lookup` supplying `name.value` for each property. Unknown names
/// evaluate as `undefined`; a malformed expression evaluates as `true` so the control stays
/// reachable.
pub fn holds(expr: &str, lookup: &dyn Fn(&str) -> Option<Value>) -> bool {
    match evaluate(expr, lookup) {
        Ok(v) => truthy(&v),
        Err(_) => true,
    }
}

pub fn evaluate(expr: &str, lookup: &dyn Fn(&str) -> Option<Value>) -> Result<Value, String> {
    let tokens = tokenize(expr)?;
    let mut p = Parser {
        tokens,
        pos: 0,
        lookup,
    };
    let v = p.or()?;
    if p.pos != p.tokens.len() {
        return Err(format!("unexpected token {:?}", p.tokens[p.pos]));
    }
    Ok(v)
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Ident(String),
    Num(f64),
    Str(String),
    Op(&'static str),
    Open,
    Close,
}

const OPS: &[&str] = &[
    "===", "!==", "==", "!=", "<=", ">=", "&&", "||", "<", ">", "!",
];

fn tokenize(s: &str) -> Result<Vec<Token>, String> {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i] as char;
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '(' {
            out.push(Token::Open);
            i += 1;
            continue;
        }
        if c == ')' {
            out.push(Token::Close);
            i += 1;
            continue;
        }
        if c == '"' || c == '\'' {
            let start = i + 1;
            let mut j = start;
            while j < b.len() && b[j] as char != c {
                j += 1;
            }
            if j >= b.len() {
                return Err("unterminated string".into());
            }
            out.push(Token::Str(s[start..j].to_string()));
            i = j + 1;
            continue;
        }
        if c.is_ascii_digit() || (c == '.' && b.get(i + 1).is_some_and(|n| n.is_ascii_digit())) {
            let start = i;
            while i < b.len() && ((b[i] as char).is_ascii_digit() || b[i] as char == '.') {
                i += 1;
            }
            let n: f64 = s[start..i].parse().map_err(|_| "bad number".to_string())?;
            out.push(Token::Num(n));
            continue;
        }
        if c.is_ascii_alphabetic() || c == '_' || c == '$' {
            let start = i;
            while i < b.len()
                && ((b[i] as char).is_ascii_alphanumeric() || matches!(b[i] as char, '_' | '$' | '.'))
            {
                i += 1;
            }
            out.push(Token::Ident(s[start..i].to_string()));
            continue;
        }
        if let Some(op) = OPS.iter().find(|op| s[i..].starts_with(*op)) {
            out.push(Token::Op(op));
            i += op.len();
            continue;
        }
        return Err(format!("unexpected character '{c}'"));
    }
    Ok(out)
}

struct Parser<'a> {
    tokens: Vec<Token>,
    pos: usize,
    lookup: &'a dyn Fn(&str) -> Option<Value>,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn eat(&mut self, op: &str) -> bool {
        if self.peek() == Some(&Token::Op(match OPS.iter().find(|o| **o == op) {
            Some(o) => o,
            None => return false,
        })) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn or(&mut self) -> Result<Value, String> {
        let mut left = self.and()?;
        while self.eat("||") {
            let right = self.and()?;
            left = if truthy(&left) { left } else { right };
        }
        Ok(left)
    }

    fn and(&mut self) -> Result<Value, String> {
        let mut left = self.equality()?;
        while self.eat("&&") {
            let right = self.equality()?;
            left = if truthy(&left) { right } else { left };
        }
        Ok(left)
    }

    fn equality(&mut self) -> Result<Value, String> {
        let mut left = self.relational()?;
        loop {
            if self.eat("===") {
                let r = self.relational()?;
                left = Value::Bool(strict_equal(&left, &r));
            } else if self.eat("!==") {
                let r = self.relational()?;
                left = Value::Bool(!strict_equal(&left, &r));
            } else if self.eat("==") {
                let r = self.relational()?;
                left = Value::Bool(loose_equal(&left, &r));
            } else if self.eat("!=") {
                let r = self.relational()?;
                left = Value::Bool(!loose_equal(&left, &r));
            } else {
                return Ok(left);
            }
        }
    }

    fn relational(&mut self) -> Result<Value, String> {
        let mut left = self.unary()?;
        loop {
            let op = if self.eat("<=") {
                "<="
            } else if self.eat(">=") {
                ">="
            } else if self.eat("<") {
                "<"
            } else if self.eat(">") {
                ">"
            } else {
                return Ok(left);
            };
            let right = self.unary()?;
            let (a, b) = (to_number(&left), to_number(&right));
            left = Value::Bool(match (a, b) {
                (Some(a), Some(b)) => match op {
                    "<=" => a <= b,
                    ">=" => a >= b,
                    "<" => a < b,
                    _ => a > b,
                },
                _ => match (left.as_str(), right.as_str()) {
                    (Some(a), Some(b)) => match op {
                        "<=" => a <= b,
                        ">=" => a >= b,
                        "<" => a < b,
                        _ => a > b,
                    },
                    _ => false,
                },
            });
        }
    }

    fn unary(&mut self) -> Result<Value, String> {
        if self.eat("!") {
            let v = self.unary()?;
            return Ok(Value::Bool(!truthy(&v)));
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<Value, String> {
        let tok = self.peek().cloned().ok_or("unexpected end of expression")?;
        self.pos += 1;
        match tok {
            Token::Num(n) => Ok(serde_json::Number::from_f64(n)
                .map(Value::Number)
                .unwrap_or(Value::Null)),
            Token::Str(s) => Ok(Value::String(s)),
            Token::Open => {
                let v = self.or()?;
                if self.peek() != Some(&Token::Close) {
                    return Err("missing ')'".into());
                }
                self.pos += 1;
                Ok(v)
            }
            Token::Ident(name) => Ok(match name.as_str() {
                "true" => Value::Bool(true),
                "false" => Value::Bool(false),
                "null" | "undefined" => Value::Null,
                _ => {
                    let (prop, field) = name.split_once('.').unwrap_or((name.as_str(), "value"));
                    match (self.lookup)(prop) {
                        Some(v) if field == "value" => v,
                        Some(Value::Object(o)) => o.get(field).cloned().unwrap_or(Value::Null),
                        _ => Value::Null,
                    }
                }
            }),
            Token::Close | Token::Op(_) => Err(format!("unexpected token {tok:?}")),
        }
    }
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

fn to_number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        Value::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                Some(0.0)
            } else {
                t.parse().ok()
            }
        }
        Value::Null => None,
        _ => None,
    }
}

fn strict_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        _ => std::mem::discriminant(a) == std::mem::discriminant(b) && a == b,
    }
}

/// JavaScript `==`: numbers, numeric strings and booleans compare by number.
fn loose_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Null, _) | (_, Value::Null) => false,
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Bool(_), _) | (_, Value::Bool(_)) | (Value::Number(_), _) | (_, Value::Number(_)) => {
            match (to_number(a), to_number(b)) {
                (Some(x), Some(y)) => x == y,
                _ => false,
            }
        }
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn env(name: &str) -> Option<Value> {
        Some(match name {
            "showclock" => json!(true),
            "count" => json!(3),
            "style" => json!("plain"),
            "ratio" => json!("2.5"),
            "off" => json!(false),
            _ => return None,
        })
    }

    #[test]
    fn evaluates_wallpaper_engine_conditions() {
        for (expr, expected) in [
            ("showclock.value == true", true),
            ("showclock.value == false", false),
            ("off.value == false", true),
            ("count.value > 2", true),
            ("count.value >= 4", false),
            ("count.value == \"3\"", true),
            ("count.value === \"3\"", false),
            ("style.value == 'plain'", true),
            ("style.value != 'plain'", false),
            ("ratio.value > 2 && count.value < 5", true),
            ("ratio.value > 3 || count.value < 5", true),
            ("!(count.value == 3)", false),
            ("!off.value", true),
            ("missing.value == true", false),
            ("missing.value == undefined", true),
            ("showclock", true),
            ("count.value == 3.0", true),
        ] {
            assert_eq!(holds(expr, &env), expected, "{expr}");
        }
    }

    #[test]
    fn malformed_conditions_keep_the_control_visible() {
        for expr in ["count.value ==", "(count.value", "count.value >", "'open", "@"] {
            assert!(evaluate(expr, &env).is_err(), "{expr}");
            assert!(holds(expr, &env), "{expr}");
        }
    }
}
