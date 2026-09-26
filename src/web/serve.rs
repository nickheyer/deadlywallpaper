//! Loopback HTTP server for wallpapers hosted by an external renderer. It serves a web
//! wallpaper's files and streams engine events (properties, pause and volume, pointer motion,
//! audio spectra) to the page as server-sent events.

use crate::content::ContentId;
use crate::error::{Error, Result};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct Entry {
    root: Option<PathBuf>,
    token: String,
    subscribers: Vec<TcpStream>,
    /// Events replayed to every new subscriber, keyed so later values replace earlier ones.
    retained: Vec<(String, String, String)>,
}

#[derive(Clone)]
pub struct Server {
    port: u16,
    entries: Arc<Mutex<HashMap<ContentId, Entry>>>,
}

impl Server {
    /// Bind an ephemeral loopback port and serve on background threads.
    pub fn start() -> Result<Server> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|e| Error::Web(format!("bind loopback server: {e}")))?;
        let port = listener.local_addr().map_err(|e| Error::Web(e.to_string()))?.port();
        let entries: Arc<Mutex<HashMap<ContentId, Entry>>> = Arc::default();
        let accept_entries = entries.clone();
        std::thread::Builder::new()
            .name("web-serve".into())
            .spawn(move || {
                for conn in listener.incoming() {
                    match conn {
                        Ok(stream) => {
                            let entries = accept_entries.clone();
                            let _ = std::thread::Builder::new().name("web-conn".into()).spawn(move || handle(stream, &entries));
                        }
                        Err(e) => log::warn!("web serve accept: {e}"),
                    }
                }
            })
            .map_err(|e| Error::Web(e.to_string()))?;
        let ping_entries = entries.clone();
        std::thread::Builder::new()
            .name("web-serve-ping".into())
            .spawn(move || {
                loop {
                    std::thread::sleep(Duration::from_secs(15));
                    if let Ok(mut all) = ping_entries.lock() {
                        for entry in all.values_mut() {
                            entry.subscribers.retain_mut(|s| s.write_all(b": ping\n\n").and_then(|_| s.flush()).is_ok());
                        }
                    }
                }
            })
            .map_err(|e| Error::Web(e.to_string()))?;
        log::info!("wallpaper web server on 127.0.0.1:{port}");
        Ok(Server { port, entries })
    }

    /// Register a content; `root` is the directory its files are served from. Returns the
    /// token its page must present to subscribe to events.
    pub fn register(&self, id: ContentId, root: Option<PathBuf>) -> String {
        let token = format!("{}{}", crate::paths::nonce(), crate::paths::nonce());
        if let Ok(mut all) = self.entries.lock() {
            all.insert(id, Entry { root, token: token.clone(), subscribers: Vec::new(), retained: Vec::new() });
        }
        token
    }

    pub fn unregister(&self, id: ContentId) {
        if let Ok(mut all) = self.entries.lock() {
            all.remove(&id);
        }
    }

    /// Send `event` with `data` to the content's page. With `retain`, the event is also kept
    /// under that key and replayed to pages that connect later.
    pub fn push(&self, id: ContentId, event: &str, data: &str, retain: Option<&str>) {
        let Ok(mut all) = self.entries.lock() else { return };
        let Some(entry) = all.get_mut(&id) else { return };
        if let Some(key) = retain {
            entry.retained.retain(|(k, _, _)| k != key);
            entry.retained.push((key.to_string(), event.to_string(), data.to_string()));
        }
        let frame = format_event(event, data);
        entry.subscribers.retain_mut(|s| s.write_all(frame.as_bytes()).and_then(|_| s.flush()).is_ok());
    }

    pub fn page_url(&self, id: ContentId, relative: &str) -> String {
        let rel = relative.replace('\\', "/");
        format!("http://127.0.0.1:{}/c/{}/{}", self.port, id, rel.trim_start_matches('/'))
    }

    pub fn events_url(&self, id: ContentId, token: &str) -> String {
        format!("http://127.0.0.1:{}/c/{}/__events?token={}", self.port, id, token)
    }
}

fn format_event(event: &str, data: &str) -> String {
    let mut s = String::with_capacity(data.len() + event.len() + 16);
    s.push_str("event: ");
    s.push_str(event);
    s.push('\n');
    for line in data.split('\n') {
        s.push_str("data: ");
        s.push_str(line);
        s.push('\n');
    }
    s.push('\n');
    s
}

fn handle(mut stream: TcpStream, entries: &Arc<Mutex<HashMap<ContentId, Entry>>>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let Ok(peer) = stream.try_clone() else { return };
    let mut reader = BufReader::new(peer);
    let mut line = String::new();
    if reader.read_line(&mut line).unwrap_or(0) == 0 {
        return;
    }
    let mut parts = line.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/"));
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).unwrap_or(0) == 0 || header.trim().is_empty() {
            break;
        }
    }
    if method != "GET" && method != "HEAD" {
        let _ = respond(&mut stream, 405, "text/plain", b"method not allowed");
        return;
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let Some(rest) = path.strip_prefix("/c/") else {
        let _ = respond(&mut stream, 404, "text/plain", b"not found");
        return;
    };
    let Some((id, rel)) = rest.split_once('/') else {
        let _ = respond(&mut stream, 404, "text/plain", b"not found");
        return;
    };
    let Ok(id) = id.parse::<ContentId>() else {
        let _ = respond(&mut stream, 404, "text/plain", b"not found");
        return;
    };
    if rel == "__events" {
        subscribe(stream, entries, id, query);
        return;
    }
    let root = entries.lock().ok().and_then(|all| all.get(&id).and_then(|e| e.root.clone()));
    let Some(root) = root else {
        let _ = respond(&mut stream, 404, "text/plain", b"not found");
        return;
    };
    let Some(file) = resolve(&root, &percent_decode(rel)) else {
        let _ = respond(&mut stream, 403, "text/plain", b"forbidden");
        return;
    };
    match std::fs::read(&file) {
        Ok(body) => {
            let mime = mime_guess::from_path(&file).first_or_octet_stream();
            let ct = if mime.type_() == mime_guess::mime::TEXT || mime.subtype() == mime_guess::mime::JAVASCRIPT || mime.subtype() == mime_guess::mime::JSON {
                format!("{mime}; charset=utf-8")
            } else {
                mime.to_string()
            };
            let _ = respond(&mut stream, 200, &ct, if method == "HEAD" { &[] } else { &body });
        }
        Err(_) => {
            let _ = respond(&mut stream, 404, "text/plain", b"not found");
        }
    }
}

fn subscribe(mut stream: TcpStream, entries: &Arc<Mutex<HashMap<ContentId, Entry>>>, id: ContentId, query: &str) {
    let token = query.split('&').find_map(|kv| kv.strip_prefix("token=")).unwrap_or("");
    let Ok(mut all) = entries.lock() else { return };
    let Some(entry) = all.get_mut(&id) else {
        let _ = respond(&mut stream, 404, "text/plain", b"not found");
        return;
    };
    if token.is_empty() || token != entry.token {
        let _ = respond(&mut stream, 403, "text/plain", b"forbidden");
        return;
    }
    let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\nAccess-Control-Allow-Origin: *\r\n\r\n";
    if stream.write_all(head.as_bytes()).is_err() {
        return;
    }
    let mut replay = String::from(": connected\n\n");
    for (_, event, data) in &entry.retained {
        replay.push_str(&format_event(event, data));
    }
    if stream.write_all(replay.as_bytes()).and_then(|_| stream.flush()).is_err() {
        return;
    }
    let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
    entry.subscribers.push(stream);
}

fn respond(stream: &mut TcpStream, status: u16, content_type: &str, body: &[u8]) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Error",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-cache\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

/// Join a request path onto the wallpaper root, refusing anything that escapes it.
fn resolve(root: &Path, rel: &str) -> Option<PathBuf> {
    let mut path = root.to_path_buf();
    for part in rel.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return None;
        }
        path.push(part);
    }
    Some(path)
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if let (Some(h), Some(l)) = (hex(bytes.get(i + 1).copied()), hex(bytes.get(i + 2).copied())) {
                out.push(h << 4 | l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(b: Option<u8>) -> Option<u8> {
    match b? {
        c @ b'0'..=b'9' => Some(c - b'0'),
        c @ b'a'..=b'f' => Some(c - b'a' + 10),
        c @ b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    /// Accumulate reads until `needle` arrives; the server writes headers and events in
    /// separate segments.
    fn read_until(s: &mut TcpStream, needle: &str) -> String {
        let mut text = String::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let mut buf = [0u8; 1024];
        while !text.contains(needle) {
            assert!(std::time::Instant::now() < deadline, "timed out waiting for {needle:?}; got {text:?}");
            let n = s.read(&mut buf).unwrap();
            assert!(n > 0, "connection closed while waiting for {needle:?}; got {text:?}");
            text.push_str(&String::from_utf8_lossy(&buf[..n]));
        }
        text
    }

    #[test]
    fn serves_files_and_streams_events() {
        let server = Server::start().unwrap();
        let root = std::env::temp_dir().join(format!("deadlywp-serve-{}", crate::paths::nonce()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("index.html"), "<html>hi</html>").unwrap();
        let token = server.register(7, Some(root.clone()));

        let mut s = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
        s.write_all(b"GET /c/7/index.html HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        let mut body = String::new();
        s.read_to_string(&mut body).unwrap();
        assert!(body.starts_with("HTTP/1.1 200"), "{body}");
        assert!(body.ends_with("<html>hi</html>"));

        let mut s = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
        s.write_all(b"GET /c/7/../etc/passwd HTTP/1.1\r\n\r\n").unwrap();
        let mut body = String::new();
        s.read_to_string(&mut body).unwrap();
        assert!(body.starts_with("HTTP/1.1 403"));

        server.push(7, "prop", r#"{"name":"hue","value":3}"#, Some("prop:hue"));
        let mut s = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        s.write_all(format!("GET /c/7/__events?token={token} HTTP/1.1\r\n\r\n").as_bytes()).unwrap();
        let mut text = read_until(&mut s, "event: prop\ndata: {\"name\":\"hue\",\"value\":3}\n\n");
        assert!(text.contains("text/event-stream"), "{text}");
        server.push(7, "audio", "[1,2]", None);
        text = read_until(&mut s, "event: audio\ndata: [1,2]\n\n");
        assert!(text.ends_with("event: audio\ndata: [1,2]\n\n"), "{text}");

        let mut s = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
        s.write_all(b"GET /c/7/__events?token=wrong HTTP/1.1\r\n\r\n").unwrap();
        let mut body = String::new();
        s.read_to_string(&mut body).unwrap();
        assert!(body.starts_with("HTTP/1.1 403"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
