//! Minimal LSP (Language Server Protocol) client over stdio.
//!
//! * One server process per language (configurable in Settings / settings.json
//!   under `"lsp": { "rs": "rust-analyzer", "py": "pylsp", … }`).
//! * Full-text sync (didOpen/didChange), publishDiagnostics, completion and
//!   definition — enough to make the editor feel like a real IDE.
//! * All LSP positions are (line, UTF-16 col) — conversions via crate::util.
//! * 100% Rust: std::process + serde_json only.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

// ---------------------------------------------------------------- data

#[derive(Clone, Debug)]
pub struct LspDiag {
    pub range: (usize, usize), // flat char range
    pub severity: u8,          // 1=err 2=warn 3=info 4=hint
    pub message: String,
}

#[derive(Clone, Debug)]
pub struct LspCompletion {
    pub label: String,
    pub detail: String,
    pub kind: String,
    pub insert: String,
}

pub struct LspServer {
    pub ext: String,
    pub name: String,
    child: Child,
    stdin: Arc<Mutex<std::process::ChildStdin>>,
    inbox: Arc<Mutex<Vec<Value>>>,
    alive: Arc<AtomicBool>,
    next_id: Arc<AtomicU64>,
    initialized: bool,
    /// notifications received before `initialize` completed (didOpen etc.).
    /// Most servers DROP messages that arrive before initialize — we buffer
    /// them here and flush right after the handshake instead.
    pending: Vec<Value>,
}

// ---------------------------------------------------------------- framing

fn write_msg(stdin: &mut std::process::ChildStdin, v: &Value) -> std::io::Result<()> {
    let body = serde_json::to_string(v).unwrap_or_default();
    write!(stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body)?;
    stdin.flush()
}

// ---------------------------------------------------------------- hub

#[derive(Default)]
pub struct LspHub {
    pub servers: HashMap<String, Arc<Mutex<Option<LspServer>>>>,
    /// path -> diagnostics for open files
    pub diags: HashMap<std::path::PathBuf, Vec<LspDiag>>,
    /// request id -> completions (arrive async)
    pub completions: HashMap<u64, Vec<LspCompletion>>,
    /// request id -> definition locations
    pub definitions: HashMap<u64, Vec<(String, u32, u32)>>,
    pub status: String,
}

impl LspHub {
    /// Get (or spawn) the server for an extension.
    pub fn server_for(&mut self, ext: &str, command: &str, root: Option<&std::path::Path>) -> Option<Arc<Mutex<Option<LspServer>>>> {
        if let Some(s) = self.servers.get(ext) {
            let alive = s.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|srv| srv.alive.load(Ordering::SeqCst)).unwrap_or(false);
            if alive {
                return Some(Arc::clone(s));
            }
        }
        if command.trim().is_empty() {
            return None;
        }
        let mut parts = command.split_whitespace();
        let prog = parts.next()?;
        let args: Vec<&str> = parts.collect();
        let mut cmd = Command::new(prog);
        cmd.args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some(r) = root {
            cmd.current_dir(r);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(_) => {
                self.status = format!("LSP '{prog}' not found — install it or change Settings");
                return None;
            }
        };
        let stdin = child.stdin.take().expect("lsp stdin");
        let stdout = child.stdout.take().expect("lsp stdout");

        let inbox: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
        let alive = Arc::new(AtomicBool::new(true));
        {
            let inbox = Arc::clone(&inbox);
            let alive = Arc::clone(&alive);
            std::thread::spawn(move || {
                let mut r = BufReader::new(stdout);
                loop {
                    // read headers
                    let mut line = String::new();
                    if r.read_line(&mut line).unwrap_or(0) == 0 {
                        break;
                    }
                    if !line.to_lowercase().starts_with("content-length:") {
                        continue;
                    }
                    let len: usize = line
                        .trim_start_matches(|c: char| !c.is_ascii_digit())
                        .trim_end_matches(['\r', '\n'])
                        .parse()
                        .unwrap_or(0);
                    if len == 0 {
                        break;
                    }
                    // sane cap: a malicious/buggy server can send a huge
                    // Content-Length and make us allocate gigabytes
                    const MAX_MSG: usize = 64 * 1024 * 1024;
                    if len > MAX_MSG {
                        break;
                    }
                    let mut buf = vec![0u8; len];
                    if r.read_exact(&mut buf).is_err() {
                        break;
                    }
                    if let Ok(v) = serde_json::from_slice::<Value>(&buf) {
                        inbox.lock().unwrap_or_else(|e| e.into_inner()).push(v);
                    }
                }
                alive.store(false, Ordering::SeqCst);
            });
        }

        let srv = LspServer {
            ext: ext.to_string(),
            name: prog.to_string(),
            child,
            stdin: Arc::new(Mutex::new(stdin)),
            inbox,
            alive: Arc::clone(&alive),
            next_id: Arc::new(AtomicU64::new(1)),
            initialized: false,
            pending: Vec::new(),
        };
        let cell = Arc::new(Mutex::new(Some(srv)));
        self.servers.insert(ext.to_string(), Arc::clone(&cell));
        Some(cell)
    }

    /// Drain incoming notifications/responses for dispatch by app.rs.
    pub fn drain(&mut self, root: Option<&std::path::Path>) -> Vec<Value> {
        let mut out = Vec::new();
        for cell in self.servers.values() {
            let mut guard = cell.lock().unwrap_or_else(|e| e.into_inner());
            let Some(srv) = guard.as_mut() else { continue };
            if !srv.initialized {
                // send initialize now (root known)
                let id = srv.next_id.fetch_add(1, Ordering::SeqCst);
                let root_uri = root.map(|r| path_to_uri(r)).unwrap_or_default();
                let msg = json!({
                    "jsonrpc": "2.0", "id": id, "method": "initialize",
                    "params": {
                        "processId": std::process::id(),
                        "rootUri": root_uri,
                        "capabilities": {
                            "textDocument": {
                                "completion": {"completionItem": {"snippetSupport": false}},
                                "synchronization": {"didChange": true, "didSave": true, "save": {"includeText": true}}
                            }
                        }
                    }
                });
                let _ = write_msg(&mut srv.stdin.lock().unwrap_or_else(|e| e.into_inner()), &msg);
                let _ = write_msg(&mut srv.stdin.lock().unwrap_or_else(|e| e.into_inner()), &json!({"jsonrpc":"2.0","method":"initialized","params":{}}));
                srv.initialized = true;
                // flush everything that was buffered before the handshake
                // (didOpen sent before initialize is DROPPED by rust-analyzer / pylsp)
                for p in std::mem::take(&mut srv.pending) {
                    let _ = write_msg(&mut srv.stdin.lock().unwrap_or_else(|e| e.into_inner()), &p);
                }
            }
            out.extend(srv.inbox.lock().unwrap_or_else(|e| e.into_inner()).drain(..));
            // reap dead servers
            if !srv.alive.load(Ordering::SeqCst) {
                self.status = format!("LSP '{}' exited", srv.name);
            }
        }
        self.servers.retain(|_, cell| {
            cell.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|s| s.alive.load(Ordering::SeqCst)).unwrap_or(false)
        });
        out
    }
}

impl Drop for LspServer {
    fn drop(&mut self) {
        // polite shutdown, then kill
        let _ = write_msg(
            &mut self.stdin.lock().unwrap_or_else(|e| e.into_inner()),
            &json!({"jsonrpc":"2.0","id":0,"method":"shutdown","params":null}),
        );
        let _ = write_msg(&mut self.stdin.lock().unwrap_or_else(|e| e.into_inner()), &json!({"jsonrpc":"2.0","method":"exit"}));
        let _ = self.child.kill();
    }
}

// ---------------------------------------------------------------- senders

pub fn did_open(srv: &Arc<Mutex<Option<LspServer>>>, path: &std::path::Path, text: &str, lang: &str) {
    let mut guard = srv.lock().unwrap_or_else(|e| e.into_inner());
    let Some(s) = guard.as_mut() else { return };
    let msg = json!({
        "jsonrpc": "2.0", "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": path_to_uri(path),
                "languageId": lang,
                "version": 1,
                "text": text,
            }
        }
    });
    if !s.initialized {
        s.pending.push(msg);
        return;
    }
    let _ = write_msg(&mut s.stdin.lock().unwrap_or_else(|e| e.into_inner()), &msg);
}

pub fn did_change(srv: &Arc<Mutex<Option<LspServer>>>, path: &std::path::Path, text: &str, version: i32) {
    let mut guard = srv.lock().unwrap_or_else(|e| e.into_inner());
    let Some(s) = guard.as_mut() else { return };
    let msg = json!({
        "jsonrpc": "2.0", "method": "textDocument/didChange",
        "params": {
            "textDocument": {"uri": path_to_uri(path), "version": version},
            "contentChanges": [{"text": text}]
        }
    });
    if !s.initialized {
        // coalesce: a didChange for the same doc replaces a queued one
        let uri = path_to_uri(path);
        if let Some(existing) = s.pending.iter_mut().find(
            |p| p.get("method").and_then(|m| m.as_str()) == Some("textDocument/didChange")
                && p.pointer("/params/textDocument/uri").and_then(|u| u.as_str()) == Some(uri.as_str()),
        ) {
            *existing = msg;
        } else {
            s.pending.push(msg);
        }
        return;
    }
    let _ = write_msg(&mut s.stdin.lock().unwrap_or_else(|e| e.into_inner()), &msg);
}

pub fn did_save(srv: &Arc<Mutex<Option<LspServer>>>, path: &std::path::Path, text: &str) {
    let mut guard = srv.lock().unwrap_or_else(|e| e.into_inner());
    let Some(s) = guard.as_mut() else { return };
    if !s.initialized {
        return; // save before handshake — the didOpen buffer already has latest text
    }
    let msg = json!({
        "jsonrpc": "2.0", "method": "textDocument/didSave",
        "params": {
            "textDocument": {"uri": path_to_uri(path)},
            "text": text,
        }
    });
    let _ = write_msg(&mut s.stdin.lock().unwrap_or_else(|e| e.into_inner()), &msg);
}

pub fn did_close(srv: &Arc<Mutex<Option<LspServer>>>, path: &std::path::Path) {
    let mut guard = srv.lock().unwrap_or_else(|e| e.into_inner());
    let Some(s) = guard.as_mut() else { return };
    let msg = json!({
        "jsonrpc": "2.0", "method": "textDocument/didClose",
        "params": {"textDocument": {"uri": path_to_uri(path)}}
    });
    let _ = write_msg(&mut s.stdin.lock().unwrap_or_else(|e| e.into_inner()), &msg);
}

pub fn request_completion(srv: &Arc<Mutex<Option<LspServer>>>, path: &std::path::Path, text: &str, char_idx: usize) -> u64 {
    let mut guard = srv.lock().unwrap_or_else(|e| e.into_inner());
    let Some(s) = guard.as_mut() else { return 0 };
    let id = s.next_id.fetch_add(1, Ordering::SeqCst);
    let (line, col) = crate::util::lsp_pos(text, char_idx);
    let msg = json!({
        "jsonrpc": "2.0", "id": id, "method": "textDocument/completion",
        "params": {
            "textDocument": {"uri": path_to_uri(path)},
            "position": {"line": line, "character": col}
        }
    });
    let _ = write_msg(&mut s.stdin.lock().unwrap_or_else(|e| e.into_inner()), &msg);
    id
}

pub fn request_definition(srv: &Arc<Mutex<Option<LspServer>>>, path: &std::path::Path, text: &str, char_idx: usize) -> u64 {
    let mut guard = srv.lock().unwrap_or_else(|e| e.into_inner());
    let Some(s) = guard.as_mut() else { return 0 };
    let id = s.next_id.fetch_add(1, Ordering::SeqCst);
    let (line, col) = crate::util::lsp_pos(text, char_idx);
    let msg = json!({
        "jsonrpc": "2.0", "id": id, "method": "textDocument/definition",
        "params": {
            "textDocument": {"uri": path_to_uri(path)},
            "position": {"line": line, "character": col}
        }
    });
    let _ = write_msg(&mut s.stdin.lock().unwrap_or_else(|e| e.into_inner()), &msg);
    id
}

// ---------------------------------------------------------------- dispatch

/// Handle one incoming message; returns Some(text) for status lines.
pub fn handle_message(hub: &mut LspHub, v: Value) {
    if let Some(method) = v.get("method").and_then(|m| m.as_str()) {
        match method {
            "textDocument/publishDiagnostics" => {
                let params = &v["params"];
                let uri = params["uri"].as_str().unwrap_or_default();
                let path = uri_to_path(uri);
                let mut list = Vec::new();
                if let Some(text) = hub_open_text(hub, &path) {
                    for d in params["diagnostics"].as_array().unwrap_or(&vec![]).iter() {
                        let (sl, sc) = (
                            d["range"]["start"]["line"].as_u64().unwrap_or(0) as u32,
                            d["range"]["start"]["character"].as_u64().unwrap_or(0) as u32,
                        );
                        let (el, ec) = (
                            d["range"]["end"]["line"].as_u64().unwrap_or(sl as u64) as u32,
                            d["range"]["end"]["character"].as_u64().unwrap_or(sc as u64) as u32,
                        );
                        let s = crate::util::lsp_pos_to_char(&text, sl, sc);
                        let e = crate::util::lsp_pos_to_char(&text, el, ec).max(s);
                        list.push(LspDiag {
                            range: (s, e),
                            severity: d["severity"].as_u64().unwrap_or(1) as u8,
                            message: d["message"].as_str().unwrap_or_default().to_string(),
                        });
                    }
                }
                hub.diags.insert(path, list);
            }
            _ => {}
        }
        return;
    }
    // responses
    let Some(id) = v.get("id").and_then(|i| i.as_u64()) else { return };
    let Some(result) = v.get("result") else { return };
    // NOTE: a definition response can be an ARRAY of locations — check for
    // locations FIRST, otherwise array results get misrouted into completions
    // and goto-definition silently breaks for multi-location results.
    if let Some(locs) = as_locations(result).filter(|l| !l.is_empty()) {
        hub.definitions.insert(id, locs);
        return;
    }
    if hub.completions.contains_key(&id)
        || v.pointer("/result/items").is_some()
        || result.is_array()
        || result.get("items").is_some()
    {
        let mut items = Vec::new();
        let arr: Vec<Value> = if let Some(a) = result.as_array() {
            a.clone()
        } else if let Some(a) = result.get("items").and_then(|x| x.as_array()) {
            a.clone()
        } else {
            vec![]
        };
        for it in arr {
            let label = it["label"].as_str().unwrap_or("").to_string();
            if label.is_empty() {
                continue;
            }
            let detail = it["detail"].as_str().unwrap_or("").to_string();
            let kind = match it["kind"].as_u64().unwrap_or(0) {
                1 => "text", 2 => "method", 3 => "func", 4 => "ctor", 5 => "field",
                6 => "var", 7 => "class", 8 => "iface", 9 => "mod", 10 => "prop",
                12 => "enum", 14 => "kw", 15 => "snip", 21 => "const", _ => "item",
            }
            .to_string();
            let insert = it["insertText"]
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| label.clone());
            items.push(LspCompletion { label, detail, kind, insert });
        }
        hub.completions.insert(id, items);
    } else if let Some(locs) = as_locations(result) {
        hub.definitions.insert(id, locs);
    }
}

fn as_locations(v: &Value) -> Option<Vec<(String, u32, u32)>> {
    let single = |x: &Value| -> Option<(String, u32, u32)> {
        let uri = x["uri"].as_str()?;
        let line = x["range"]["start"]["line"].as_u64()? as u32;
        let col = x["range"]["start"]["character"].as_u64()? as u32;
        Some((uri_to_path(uri).display().to_string(), line, col))
    };
    if let Some(l) = single(v) {
        return Some(vec![l]);
    }
    v.as_array().map(|a| a.iter().filter_map(single).collect())
}

/// The hub doesn't store file text; app.rs injects the open doc's text before
/// diagnostics are converted (we look it up via a side-channel).
fn hub_open_text(_hub: &LspHub, _path: &std::path::Path) -> Option<String> {
    // replaced at call site via OPEN_DOC_TEXT global below
    OPEN_DOC_TEXT.with(|c| c.borrow().get(_path).cloned())
}

thread_local! {
    static OPEN_DOC_TEXT: std::cell::RefCell<HashMap<std::path::PathBuf, String>> =
        std::cell::RefCell::new(HashMap::new());
}

/// Publish the current text of an open doc so diagnostics can be converted.
pub fn set_doc_text(path: &std::path::Path, text: Option<&str>) {
    OPEN_DOC_TEXT.with(|c| {
        if let Some(t) = text {
            c.borrow_mut().insert(path.to_path_buf(), t.to_string());
        } else {
            c.borrow_mut().remove(path);
        }
    });
}

// ---------------------------------------------------------------- uri helpers

pub fn path_to_uri(p: &std::path::Path) -> String {
    let s = p.display().to_string().replace('\\', "/");
    let drive = if let Some(rest) = s.strip_prefix('/') {
        // /C/... -> C:/...
        let mut ch = rest.chars();
        match (ch.next(), ch.next()) {
            (Some(c), Some('/')) => format!("{c}:{}", &rest[1..]),
            _ => s.clone(),
        }
    } else {
        s.clone()
    };
    let mut enc = String::from("file:///");
    for b in drive.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' => {
                enc.push(*b as char)
            }
            _ => enc.push_str(&format!("%{b:02X}")),
        }
    }
    enc
}

pub fn uri_to_path(uri: &str) -> std::path::PathBuf {
    let s = uri.strip_prefix("file://").unwrap_or(uri);
    let s = s.strip_prefix('/').unwrap_or(s);
    // percent decode
    let mut out = String::with_capacity(s.len());
    let bytes: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == '%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&format!("{}{}", bytes[i + 1], bytes[i + 2]), 16) {
                out.push(b as char);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    std::path::PathBuf::from(out)
}
