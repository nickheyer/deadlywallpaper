//! plasmashell's scripting interface over D-Bus. Every script prints either `OK|<value>` or
//! `ERR|<message>`, so results and failures travel the same way.

use crate::error::{Error, Result};
use std::sync::mpsc::{RecvTimeoutError, Sender, channel};
use std::time::Duration;
use zbus::blocking::{Connection, Proxy};

pub const PLUGIN: &str = "org.deadlywp.live";
const GROUP: &str = r#"["Wallpaper", "org.deadlywp.live", "General"]"#;

type Callback = Box<dyn FnOnce(Result<String>) + Send + 'static>;

struct Job {
    script: String,
    done: Option<Callback>,
}

/// Serialized access to `org.kde.PlasmaShell.evaluateScript` on a worker thread, so a slow
/// plasmashell never stalls the daemon's main loop.
#[derive(Clone)]
pub struct Client {
    tx: Sender<Job>,
}

pub fn available() -> bool {
    let Ok(conn) = Connection::session() else {
        return false;
    };
    let Ok(dbus) = zbus::blocking::fdo::DBusProxy::new(&conn) else {
        return false;
    };
    let Ok(name) = zbus::names::BusName::try_from("org.kde.plasmashell") else {
        return false;
    };
    dbus.name_has_owner(name).unwrap_or(false)
}

impl Client {
    pub fn connect() -> Result<Client> {
        let conn =
            Connection::session().map_err(|e| Error::Platform(format!("session bus: {e}")))?;
        let proxy = Proxy::new(
            &conn,
            "org.kde.plasmashell",
            "/PlasmaShell",
            "org.kde.PlasmaShell",
        )
        .map_err(|e| Error::Platform(format!("plasmashell: {e}")))?;
        let probe: String = proxy
            .call("evaluateScript", &("print('OK|ready')",))
            .map_err(|e| Error::Platform(format!("plasmashell scripting: {e}")))?;
        parse(&probe)?;
        let (tx, rx) = channel::<Job>();
        std::thread::Builder::new()
            .name("plasma-script".into())
            .spawn(move || {
                let _keep = conn;
                for job in rx {
                    let result: Result<String> = proxy
                        .call::<_, _, String>("evaluateScript", &(job.script.as_str(),))
                        .map_err(|e| Error::Platform(format!("plasmashell script: {e}")))
                        .and_then(|out| parse(&out));
                    match job.done {
                        Some(done) => done(result),
                        None => {
                            if let Err(e) = result {
                                log::warn!("{e}");
                            }
                        }
                    }
                }
            })
            .map_err(|e| Error::Platform(e.to_string()))?;
        Ok(Client { tx })
    }

    /// Run a script; failures are logged.
    pub fn eval(&self, script: String) {
        let _ = self.tx.send(Job { script, done: None });
    }

    /// Run a script and hand its result to `done` on the worker thread.
    pub fn eval_then(&self, script: String, done: impl FnOnce(Result<String>) + Send + 'static) {
        let _ = self.tx.send(Job {
            script,
            done: Some(Box::new(done)),
        });
    }

    /// Run a script and wait for its result.
    pub fn eval_sync(&self, script: String, timeout: Duration) -> Result<String> {
        let (tx, rx) = channel();
        self.tx
            .send(Job {
                script,
                done: Some(Box::new(move |r| {
                    let _ = tx.send(r);
                })),
            })
            .map_err(|_| Error::Platform("plasmashell worker is gone".into()))?;
        match rx.recv_timeout(timeout) {
            Ok(r) => r,
            Err(RecvTimeoutError::Timeout) => Err(Error::Platform(format!(
                "plasmashell did not answer within {}s",
                timeout.as_secs()
            ))),
            Err(RecvTimeoutError::Disconnected) => {
                Err(Error::Platform("plasmashell worker is gone".into()))
            }
        }
    }
}

fn parse(out: &str) -> Result<String> {
    let t = out.trim();
    if let Some(rest) = t.strip_prefix("OK|") {
        return Ok(rest.to_string());
    }
    if let Some(rest) = t.strip_prefix("ERR|") {
        return Err(Error::Platform(format!("plasmashell: {rest}")));
    }
    Err(Error::Platform(format!(
        "plasmashell returned an unexpected result: {t}"
    )))
}

/// A value written into the wallpaper's configuration group.
#[derive(Clone, Debug)]
pub enum Val {
    Str(String),
    Int(i64),
    Num(f64),
    Bool(bool),
}

impl Val {
    fn js(&self) -> String {
        match self {
            Val::Str(s) => js_str(s),
            Val::Int(i) => i.to_string(),
            Val::Num(n) => {
                if n.is_finite() {
                    format!("{n}")
                } else {
                    "0".into()
                }
            }
            Val::Bool(b) => b.to_string(),
        }
    }
}

pub fn js_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

fn writes(values: &[(&str, Val)]) -> String {
    values
        .iter()
        .map(|(k, v)| format!("d.writeConfig({}, {});", js_str(k), v.js()))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Switch a containment to the live wallpaper plugin (when it is not on it already), write
/// `values`, then write the generation last so the wallpaper reloads once with everything in
/// place. Prints the plugin that was active before.
pub fn apply(containment: i32, values: &[(&str, Val)], generation: i64) -> String {
    format!(
        r#"(function() {{ try {{ var d = desktopById({containment}); if (!d) {{ print("ERR|Plasma has no desktop containment {containment}"); return; }} var prev = d.wallpaperPlugin; if (prev !== {plugin}) {{ d.wallpaperPlugin = {plugin}; }} d.currentConfigGroup = {GROUP}; {w} d.writeConfig("Generation", {generation}); print("OK|" + prev); }} catch (e) {{ print("ERR|" + e); }} }})();"#,
        plugin = js_str(PLUGIN),
        w = writes(values),
    )
}

/// Write values into the live wallpaper's configuration without reloading it.
pub fn write(containment: i32, values: &[(&str, Val)]) -> String {
    format!(
        r#"(function() {{ try {{ var d = desktopById({containment}); if (!d) {{ print("ERR|Plasma has no desktop containment {containment}"); return; }} d.currentConfigGroup = {GROUP}; {w} print("OK|"); }} catch (e) {{ print("ERR|" + e); }} }})();"#,
        w = writes(values),
    )
}

/// Read keys from the live wallpaper's configuration; prints a JSON object.
pub fn read(containment: i32, keys: &[&str]) -> String {
    let reads = keys
        .iter()
        .map(|k| format!("{}: String(d.readConfig({}, \"\"))", js_str(k), js_str(k)))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r#"(function() {{ try {{ var d = desktopById({containment}); if (!d) {{ print("ERR|Plasma has no desktop containment {containment}"); return; }} d.currentConfigGroup = {GROUP}; print("OK|" + JSON.stringify({{ plugin: d.wallpaperPlugin, {reads} }})); }} catch (e) {{ print("ERR|" + e); }} }})();"#
    )
}

/// Hand a containment back to the plugin it used before, if it still shows ours.
pub fn restore(containment: i32, previous: &str) -> String {
    format!(
        r#"(function() {{ try {{ var d = desktopById({containment}); if (!d) {{ print("OK|gone"); return; }} if (d.wallpaperPlugin === {plugin}) {{ d.wallpaperPlugin = {prev}; }} print("OK|" + d.wallpaperPlugin); }} catch (e) {{ print("ERR|" + e); }} }})();"#,
        plugin = js_str(PLUGIN),
        prev = js_str(previous),
    )
}

/// Every desktop containment with its screen geometry (screen -1 = not on the current activity).
pub fn containments() -> String {
    r#"(function() { try { var out = []; var ds = desktops(); for (var i = 0; i < ds.length; i++) { var d = ds[i]; var g = d.screen >= 0 ? screenGeometry(d.screen) : null; out.push({ id: d.id, screen: d.screen, plugin: d.wallpaperPlugin, x: g ? g.x : 0, y: g ? g.y : 0, w: g ? g.width : 0, h: g ? g.height : 0 }); } print("OK|" + JSON.stringify(out)); } catch (e) { print("ERR|" + e); } })();"#.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_escape_values_and_order_generation_last() {
        let s = apply(
            3,
            &[
                ("Source", Val::Str("file:///a \"b\".mp4".into())),
                ("Volume", Val::Num(0.5)),
                ("Paused", Val::Bool(false)),
            ],
            42,
        );
        assert!(s.contains(r#"d.writeConfig("Source", "file:///a \"b\".mp4");"#));
        assert!(s.contains(r#"d.writeConfig("Volume", 0.5);"#));
        let generation_at = s.find("\"Generation\", 42").unwrap();
        assert!(generation_at > s.find("\"Paused\"").unwrap());
        assert!(parse("OK|org.kde.image\n").is_ok_and(|v| v == "org.kde.image"));
        assert!(parse("ERR|boom").is_err());
        assert!(parse("garbage").is_err());
    }
}
