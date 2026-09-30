//! Valve's KeyValues text format (`libraryfolders.vdf`, `appworkshop_*.acf`).

#[cfg(test)]
use std::collections::BTreeMap;

/// A node of a KeyValues document: either a string or a table of children. Keys keep
/// document order through the index map; lookups are case-insensitive as in Steam.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    Value(String),
    Table(Table),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Table {
    entries: Vec<(String, Node)>,
}

impl Table {
    pub fn get(&self, key: &str) -> Option<&Node> {
        self.entries
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v)
    }

    pub fn table(&self, key: &str) -> Option<&Table> {
        match self.get(key)? {
            Node::Table(t) => Some(t),
            Node::Value(_) => None,
        }
    }

    pub fn value(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Node::Value(v) => Some(v),
            Node::Table(_) => None,
        }
    }

    pub fn u64(&self, key: &str) -> Option<u64> {
        self.value(key)?.trim().parse().ok()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &Node)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v))
    }

    pub fn tables(&self) -> impl Iterator<Item = (&str, &Table)> {
        self.entries.iter().filter_map(|(k, v)| match v {
            Node::Table(t) => Some((k.as_str(), t)),
            Node::Value(_) => None,
        })
    }

    /// Flatten string values into a map, for tests and diagnostics.
    #[cfg(test)]
    pub fn values(&self) -> BTreeMap<String, String> {
        self.entries
            .iter()
            .filter_map(|(k, v)| match v {
                Node::Value(s) => Some((k.clone(), s.clone())),
                Node::Table(_) => None,
            })
            .collect()
    }
}

/// Parse a document. The root holds the top-level keys (usually exactly one).
pub fn parse(text: &str) -> Result<Table, String> {
    let mut p = Parser {
        chars: text.chars().peekable(),
        line: 1,
    };
    let root = p.table(true)?;
    Ok(root)
}

struct Parser<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
    line: usize,
}

impl Parser<'_> {
    fn skip_space(&mut self) {
        loop {
            match self.chars.peek() {
                Some('\n') => {
                    self.line += 1;
                    self.chars.next();
                }
                Some(c) if c.is_whitespace() => {
                    self.chars.next();
                }
                Some('/') => {
                    let mut probe = self.chars.clone();
                    probe.next();
                    if probe.peek() == Some(&'/') {
                        for c in self.chars.by_ref() {
                            if c == '\n' {
                                self.line += 1;
                                break;
                            }
                        }
                    } else {
                        return;
                    }
                }
                _ => return,
            }
        }
    }

    fn token(&mut self) -> Result<Option<Token>, String> {
        self.skip_space();
        let Some(&c) = self.chars.peek() else {
            return Ok(None);
        };
        match c {
            '{' => {
                self.chars.next();
                Ok(Some(Token::Open))
            }
            '}' => {
                self.chars.next();
                Ok(Some(Token::Close))
            }
            '"' => {
                self.chars.next();
                let mut s = String::new();
                loop {
                    match self.chars.next() {
                        None => return Err(format!("line {}: unterminated string", self.line)),
                        Some('"') => break,
                        Some('\\') => match self.chars.next() {
                            Some('n') => s.push('\n'),
                            Some('t') => s.push('\t'),
                            Some('\\') => s.push('\\'),
                            Some('"') => s.push('"'),
                            Some(other) => {
                                s.push('\\');
                                s.push(other);
                            }
                            None => {
                                return Err(format!("line {}: unterminated string", self.line));
                            }
                        },
                        Some('\n') => {
                            self.line += 1;
                            s.push('\n');
                        }
                        Some(other) => s.push(other),
                    }
                }
                Ok(Some(Token::Str(s)))
            }
            _ => {
                let mut s = String::new();
                while let Some(&c) = self.chars.peek() {
                    if c.is_whitespace() || c == '{' || c == '}' || c == '"' {
                        break;
                    }
                    s.push(c);
                    self.chars.next();
                }
                Ok(Some(Token::Str(s)))
            }
        }
    }

    fn table(&mut self, root: bool) -> Result<Table, String> {
        let mut table = Table::default();
        loop {
            let key = match self.token()? {
                None if root => return Ok(table),
                None => return Err("unexpected end of document inside a table".into()),
                Some(Token::Close) if !root => return Ok(table),
                Some(Token::Close) => return Err(format!("line {}: stray '}}'", self.line)),
                Some(Token::Open) => return Err(format!("line {}: '{{' without a key", self.line)),
                Some(Token::Str(k)) => k,
            };
            let node = match self.token()? {
                Some(Token::Open) => Node::Table(self.table(false)?),
                Some(Token::Str(v)) => Node::Value(v),
                Some(Token::Close) | None => {
                    return Err(format!("line {}: key '{key}' has no value", self.line));
                }
            };
            table.entries.push((key, node));
        }
    }
}

enum Token {
    Open,
    Close,
    Str(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIBRARY: &str = r#"
"libraryfolders"
{
	"0"
	{
		"path"		"/home/nick/.local/share/Steam"
		"label"		""
		"apps"
		{
			"228980"		"1261484793"
		}
	}
	"1"
	{
		"path"		"D:\\SteamLibrary"
		"apps"
		{
			"431960"		"1234"
		}
	}
}
"#;

    #[test]
    fn parses_library_folders() {
        let root = parse(LIBRARY).unwrap();
        let folders = root.table("LibraryFolders").unwrap();
        let paths: Vec<&str> = folders
            .tables()
            .filter_map(|(_, t)| t.value("path"))
            .collect();
        assert_eq!(paths, ["/home/nick/.local/share/Steam", "D:\\SteamLibrary"]);
        assert_eq!(
            folders.table("1").unwrap().table("apps").unwrap().u64("431960"),
            Some(1234)
        );
        assert_eq!(folders.table("0").unwrap().value("label"), Some(""));
    }

    #[test]
    fn parses_unquoted_tokens_comments_and_escapes() {
        let root = parse(
            "// comment\nroot { key value \"quoted key\" \"a \\\"b\\\"\" nested { x 1 } }",
        )
        .unwrap();
        let r = root.table("root").unwrap();
        assert_eq!(r.value("key"), Some("value"));
        assert_eq!(r.value("quoted key"), Some("a \"b\""));
        assert_eq!(r.table("nested").unwrap().u64("x"), Some(1));
        assert_eq!(r.values().len(), 2);
    }

    #[test]
    fn rejects_malformed_documents() {
        assert!(parse("root {").is_err());
        assert!(parse("}").is_err());
        assert!(parse("\"open").is_err());
        assert!(parse("key").is_err());
        assert!(parse("{ }").is_err());
    }
}
