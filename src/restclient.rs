//! PLUGIN: REST Client (v1.7)
//!
//! Test APIs directly from `.http` / `.rest` files (the VS Code REST Client
//! workflow, offline-friendly):
//!
//! ```text
//! @host = https://api.example.com
//!
//! # @name users
//! ### list users
//! GET {{host}}/users
//! Authorization: Bearer {{oauth_token}}
//!
//! # @name create
//! ### create
//! POST https://httpbin.org/post
//! Content-Type: application/json
//!
//! {"name": "ada"}
//! ```
//!
//! v1.7 additions:
//!  * variables: `@name = value` file vars, `rea-rest-env.json` environments
//!    (env picker in the panel), `{{$timestamp}} / {{$uuid}} / {{$randomInt}}`
//!  * named requests (`# @name x`) + response capture:
//!    `{{create.body.$.id}}` references the JSON of a previous response
//!  * OAuth2 helper: client_credentials / password / refresh_token grants,
//!    token cached + refreshed, usable anywhere as `{{oauth_token}}`
//!
//! Requests are executed with the Windows-included `curl.exe` (handles
//! HTTP/HTTPS/TLS, proxies, timeouts) in a background thread; the UI shows
//! status, headers, body, time and size.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use eframe::egui;
use egui::{Color32, Rounding, vec2};

use crate::app::App;
use crate::theme::Theme;

// ---------------------------------------------------------------- model

#[derive(Clone)]
pub struct HttpReq {
    pub name: String,
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
    /// 1-based line of the method line (for "send" buttons in the editor)
    pub line: usize,
}

#[derive(Clone)]
pub struct RestResult {
    pub name: String,
    pub method: String,
    pub url: String,
    pub status: u16,
    pub status_text: String,
    pub time_ms: u128,
    pub size: usize,
    pub headers: String,
    pub body: String,
    pub error: Option<String>,
}

/// Background job result channel.
pub type Job = Arc<Mutex<Option<RestResult>>>;

/// v1.7: OAuth2 helper state.
#[derive(Default)]
pub struct OAuth2State {
    pub open: bool,
    pub token_url: String,
    pub client_id: String,
    pub client_secret: String,
    pub scope: String,
    /// 0 = client_credentials, 1 = password, 2 = refresh_token
    pub grant: usize,
    pub username: String,
    pub password: String,
    /// send client id/secret as Basic header instead of body fields
    pub auth_in_header: bool,
    pub token: String,
    pub refresh_token: String,
    pub token_type: String,
    pub expires_at: Option<Instant>,
    pub busy: bool,
    pub job: Option<Job>,
    pub status: String,
}

impl OAuth2State {
    pub fn token_valid(&self) -> bool {
        !self.token.is_empty()
            && self.expires_at.map(|e| Instant::now() < e).unwrap_or(true)
    }
}

#[derive(Default)]
pub struct RestState {
    pub sel: usize,
    pub results: Vec<RestResult>,
    pub job: Option<Job>,
    /// true while curl is running
    pub busy: bool,
    pub show_headers: bool,
    pub url_override: String,
    // ---- v1.7 ----
    /// named-request captures: name -> parsed response body (JSON when possible)
    pub captures: BTreeMap<String, serde_json::Value>,
    /// name -> raw body (for non-JSON responses)
    pub raw_captures: BTreeMap<String, String>,
    /// selected environment name ("" = none)
    pub env_sel: String,
    /// environment names found in rea-rest-env.json
    pub env_names: Vec<String>,
    /// path of the discovered env file (shown as tooltip)
    pub env_file: Option<PathBuf>,
    /// name of the request currently in flight (for captures)
    pub sent_name: String,
    pub oauth: OAuth2State,
}

// ---------------------------------------------------------------- .http parser

/// Parse the active document into (requests, file variables).
/// Requests are separated by `###`; `# @name foo` names the next request;
/// `@var = value` lines (anywhere) define file-level variables.
pub fn parse_file(text: &str) -> (Vec<HttpReq>, BTreeMap<String, String>) {
    let mut out: Vec<HttpReq> = Vec::new();
    let mut vars: BTreeMap<String, String> = BTreeMap::new();
    let mut name = String::new();
    let mut method = String::new();
    let mut url = String::new();
    let mut headers: Vec<(String, String)> = Vec::new();
    let mut body = String::new();
    let mut in_headers = false;
    let mut in_body = false;
    let mut line_no = 0usize;
    let mut start_line = 0usize;
    let mut pending_name = String::new();

    let mut flush = |name: &mut String, method: &mut String, url: &mut String,
                     headers: &mut Vec<(String, String)>, body: &mut String,
                     out: &mut Vec<HttpReq>, start_line: usize| {
        if !method.is_empty() && !url.is_empty() {
            out.push(HttpReq {
                name: if name.trim().is_empty() {
                    format!("{method} {url}")
                } else {
                    name.trim().to_string()
                },
                method: method.clone(),
                url: url.clone(),
                headers: headers.clone(),
                body: body.trim().to_string(),
                line: start_line,
            });
        }
        name.clear();
        method.clear();
        url.clear();
        headers.clear();
        body.clear();
    };

    for raw in text.lines() {
        line_no += 1;
        let line = raw.trim_end();
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("###") {
            flush(&mut name, &mut method, &mut url, &mut headers, &mut body, &mut out, start_line);
            name = if pending_name.is_empty() { rest.trim().to_string() } else { std::mem::take(&mut pending_name) };
            in_headers = false;
            in_body = false;
            continue;
        }
        // v1.7: # @name requestname (VS Code REST Client style)
        if let Some(n) = t.strip_prefix("#").map(|s| s.trim()) {
            if let Some(n) = n.strip_prefix("@name") {
                pending_name = n.trim().to_string();
                continue;
            }
        }
        // v1.7: @var = value
        if let Some((k, v)) = t.strip_prefix('@').and_then(|s| s.split_once('=')) {
            vars.insert(k.trim().to_string(), v.trim().to_string());
            continue;
        }
        if t.is_empty() {
            if in_headers && !method.is_empty() {
                in_headers = false;
                in_body = true;
            }
            continue;
        }
        if t.starts_with('#') || t.starts_with("//") {
            continue; // comment
        }
        if method.is_empty() {
            // request line: METHOD URL [HTTP/1.1]
            let mut it = t.split_whitespace();
            if let (Some(m), Some(u)) = (it.next(), it.next()) {
                method = m.to_uppercase();
                url = u.to_string();
                start_line = line_no;
                in_headers = true;
                in_body = false;
            }
            continue;
        }
        if in_headers {
            if let Some((k, v)) = line.split_once(':') {
                headers.push((k.trim().to_string(), v.trim().to_string()));
                continue;
            }
            // not a header → treat as body
            in_headers = false;
            in_body = true;
        }
        if in_body || !method.is_empty() {
            if !body.is_empty() {
                body.push('\n');
            }
            body.push_str(line);
        }
    }
    flush(&mut name, &mut method, &mut url, &mut headers, &mut body, &mut out, start_line);
    (out, vars)
}

/// Back-compat wrapper used by app.rs polling.
pub fn parse_requests(text: &str) -> Vec<HttpReq> {
    parse_file(text).0
}

// ---------------------------------------------------------------- environments

/// Locate + parse `rea-rest-env.json` (workspace root first, then the active
/// file's folder). Format: {"dev": {"host": "…"}, "prod": {…}}.
pub fn load_envs(app: &App) -> (Vec<String>, Option<PathBuf>, BTreeMap<String, BTreeMap<String, String>>) {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(ws) = &app.workspace {
        candidates.push(ws.join("rea-rest-env.json"));
    }
    if let Some(i) = app.active {
        if let Some(p) = &app.docs[i].path {
            if let Some(dir) = p.parent() {
                candidates.push(dir.join("rea-rest-env.json"));
            }
        }
    }
    for c in candidates {
        if let Ok(txt) = std::fs::read_to_string(&c) {
            if let Ok(serde_json::Value::Object(map)) = serde_json::from_str(&txt) {
                let mut envs = BTreeMap::new();
                let mut names = Vec::new();
                for (k, v) in map {
                    if let serde_json::Value::Object(inner) = v {
                        let mut m = BTreeMap::new();
                        for (ik, iv) in inner {
                            if let serde_json::Value::String(s) = iv {
                                m.insert(ik, s);
                            }
                        }
                        names.push(k.clone());
                        envs.insert(k, m);
                    }
                }
                names.sort();
                return (names, Some(c), envs);
            }
        }
    }
    (Vec::new(), None, BTreeMap::new())
}

// ---------------------------------------------------------------- variables

/// Tiny base64 (for Basic auth) — no external crates.
fn b64(s: &str) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let b = s.as_bytes();
    let mut out = String::new();
    for chunk in b.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

/// Pseudo-UUID v4 (no deps): nanotime + xorshift.
fn dyn_uuid() -> String {
    let mut s: u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9E3779B97F4A7C15)
        ^ (std::process::id() as u64) << 32;
    let mut hex = String::new();
    let mut next = move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s
    };
    for i in 0..32 {
        if i == 12 {
            hex.push('4');
        } else if i == 16 {
            hex.push_str(&format!("{:x}", 8 + (next() % 4)));
        } else {
            hex.push_str(&format!("{:x}", next() % 16));
        }
        if matches!(i, 7 | 11 | 15 | 19) {
            hex.push('-');
        }
    }
    hex
}

/// Navigate `$.a.b.0.c` inside a parsed JSON value.
fn json_path<'a>(v: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let p = path.trim_start_matches("$").trim_start_matches('.');
    let mut cur = v;
    if p.is_empty() {
        return Some(cur);
    }
    for seg in p.split('.') {
        match cur {
            serde_json::Value::Object(m) => cur = m.get(seg)?,
            serde_json::Value::Array(a) => {
                let idx: usize = seg.parse().ok()?;
                cur = a.get(idx)?;
            }
            _ => return None,
        }
    }
    Some(cur)
}

/// Replace `{{…}}` references in url/header/body text.
pub fn resolve_vars(app: &App, text: &str, file_vars: &BTreeMap<String, String>) -> String {
    let (env_names, _env_file, envs) = load_envs(app);
    let sel_env = if app.rest.env_sel.is_empty() {
        None
    } else {
        envs.get(&app.rest.env_sel)
    };
    let _ = env_names;

    let mut out = String::with_capacity(text.len());
    let bytes: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == '{' && i + 1 < bytes.len() && bytes[i + 1] == '{' {
            // find closing
            let mut j = i + 2;
            while j + 1 < bytes.len() && !(bytes[j] == '}' && bytes[j + 1] == '}') {
                j += 1;
            }
            if j + 1 < bytes.len() {
                let var: String = bytes[i + 2..j].iter().collect();
                let var = var.trim().to_string();
                out.push_str(&resolve_one(app, &var, file_vars, sel_env));
                i = j + 2;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

fn resolve_one(
    app: &App,
    var: &str,
    file_vars: &BTreeMap<String, String>,
    sel_env: Option<&BTreeMap<String, String>>,
) -> String {
    // dynamic variables
    if let Some(d) = var.strip_prefix('$') {
        return match d {
            "timestamp" => std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs().to_string())
                .unwrap_or("0".to_string()),
            "randomInt" => {
                let n = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.subsec_nanos() as u64)
                    .unwrap_or(42);
                (n % 1_000_000).to_string()
            }
            "uuid" => dyn_uuid(),
            other => other.to_string(),
        };
    }
    // oauth token
    if var == "oauth_token" || var == "oauth2_token" {
        return app.rest.oauth.token.clone();
    }
    if var == "oauth_refresh_token" {
        return app.rest.oauth.refresh_token.clone();
    }
    // captured responses: name.body.$.path | name.response.body.$.path | name.response.headers.$.h
    let parts: Vec<&str> = var.splitn(2, '.').collect();
    if parts.len() == 2 {
        let name = parts[0];
        let rest = parts[1];
        let norm = rest.trim_start_matches("response.").to_string();
        if let Some(cap) = app.rest.captures.get(name) {
            if let Some(p) = norm.strip_prefix("body") {
                if let Some(v) = json_path(cap, p) {
                    return match v {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                }
            }
        }
        if let Some(raw) = app.rest.raw_captures.get(name) {
            if norm.starts_with("body") {
                return raw.clone();
            }
        }
    }
    // env vars, then file vars
    if let Some(env) = sel_env {
        if let Some(v) = env.get(var) {
            return v.clone();
        }
    }
    if let Some(v) = file_vars.get(var) {
        return v.clone();
    }
    // unknown — leave the placeholder visible
    format!("{{{{{var}}}}}")
}

// ---------------------------------------------------------------- sender

/// Run the selected request with curl.exe on a background thread.
pub fn send(app: &mut App, req: &HttpReq) {
    let job: Job = Arc::new(Mutex::new(None));
    app.rest.job = Some(Arc::clone(&job));
    app.rest.busy = true;
    app.rest.sent_name = req.name.clone();

    let mut vars = file_vars_of(app);
    // allow the OAuth2 helper fields to be referenced as @-style vars too
    if !app.rest.oauth.token_url.is_empty() {
        vars.entry("token_url".into()).or_insert_with(|| app.rest.oauth.token_url.clone());
    }

    let url = resolve_vars(app, &req.url, &vars);
    let mut headers: Vec<(String, String)> = req
        .headers
        .iter()
        .map(|(k, v)| (k.clone(), resolve_vars(app, v, &vars)))
        .collect();
    let body = resolve_vars(app, &req.body, &vars);

    let mut args: Vec<String> = vec![
        "-s".into(),
        "-S".into(),
        "-i".into(), // include headers in output
        "--max-time".into(),
        "30".into(),
        "-X".into(),
        req.method.clone(),
    ];
    // SECURITY: `--` ends curl's option parsing. Without it a .http file
    // could set the URL to something like `-K c:/Users/me/secret.conf`
    // (curl config file with an `--upload-file` directive) and exfiltrate
    // local files to a remote server.
    args.push("--".into());
    args.push(url.clone());
    for (k, v) in &headers {
        args.push("-H".into());
        args.push(format!("{k}: {v}"));
    }
    if !body.is_empty() {
        if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("content-type")) {
            args.push("-H".into());
            args.push("Content-Type: application/json".into());
        }
        args.push("--data-raw".into());
        args.push(body.clone());
    }
    let _ = &mut headers;

    std::thread::spawn(move || {
        let t0 = Instant::now();
        let mut cmd = std::process::Command::new("curl.exe");
        cmd.args(&args);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let res = cmd.output();
        let time_ms = t0.elapsed().as_millis();
        let mut slot = job.lock().unwrap();
        match res {
            Ok(out) => {
                let raw = String::from_utf8_lossy(&out.stdout).into_owned();
                if !out.status.success() && raw.trim().is_empty() {
                    *slot = Some(RestResult {
                        name: String::new(),
                        method: String::new(),
                        url: String::new(),
                        status: 0,
                        status_text: String::new(),
                        time_ms,
                        size: 0,
                        headers: String::new(),
                        body: String::new(),
                        error: Some(
                            String::from_utf8_lossy(&out.stderr).trim().to_string(),
                        ),
                    });
                } else {
                    let (status, status_text, headers, body) = split_response(&raw);
                    *slot = Some(RestResult {
                        name: String::new(),
                        method: String::new(),
                        url: String::new(),
                        status,
                        status_text,
                        time_ms,
                        size: body.len(),
                        headers,
                        body,
                        error: None,
                    });
                }
            }
            Err(e) => {
                *slot = Some(RestResult {
                    name: String::new(),
                    method: String::new(),
                    url: String::new(),
                    status: 0,
                    status_text: String::new(),
                    time_ms,
                    size: 0,
                    headers: String::new(),
                    body: String::new(),
                    error: Some(format!("curl.exe not found / failed: {e}")),
                });
            }
        }
    });
}

fn file_vars_of(app: &App) -> BTreeMap<String, String> {
    app.active
        .map(|i| parse_file(&app.docs[i].text).1)
        .unwrap_or_default()
}

/// Split "HTTP/1.1 200 OK\r\nH: v\r\n\r\nbody" (handles redirect chains —
/// keeps the LAST status block).
fn split_response(raw: &str) -> (u16, String, String, String) {
    let mut status = 0u16;
    let mut status_text = String::new();
    let mut headers = String::new();
    let mut rest = raw;
    loop {
        let split = rest.find("\r\n\r\n").map(|i| (i, 4)).or_else(|| rest.find("\n\n").map(|i| (i, 2)));
        let Some((i, skip)) = split else {
            // no header/body boundary — everything is body
            headers.clear();
            return (status, status_text.clone(), headers.clone(), rest.to_string());
        };
        let head = &rest[..i];
        // status line?
        let mut ok_head = false;
        for line in head.lines() {
            if let Some(r) = line.strip_prefix("HTTP/") {
                let mut it = r.splitn(3, ' ');
                let _ver = it.next();
                if let Some(code) = it.next().and_then(|c| c.parse::<u16>().ok()) {
                    status = code;
                    status_text = it.next().unwrap_or("").to_string();
                    ok_head = true;
                    break;
                }
            }
        }
        if ok_head {
            headers = head
                .lines()
                .skip(1)
                .filter(|l| !l.trim().is_empty())
                .collect::<Vec<_>>()
                .join("\n");
        }
        rest = &rest[i + skip..];
        // if body itself starts with another HTTP/ block (redirect chain), loop
        let trimmed = rest.trim_start();
        if trimmed.starts_with("HTTP/") {
            rest = trimmed;
            continue;
        }
        return (status, status_text, headers, rest.to_string());
    }
}

/// Store a finished response under the sent request's name (for captures).
pub fn store_capture(app: &mut App, res: &RestResult) {
    let name = app.rest.sent_name.clone();
    app.rest.sent_name.clear();
    if name.is_empty() || res.error.is_some() {
        return;
    }
    app.rest.raw_captures.insert(name.clone(), res.body.clone());
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(res.body.trim()) {
        app.rest.captures.insert(name, v);
    }
}

// ---------------------------------------------------------------- oauth2

fn oauth_send(app: &mut App, form: String, header_auth: Option<String>) {
    let job: Job = Arc::new(Mutex::new(None));
    let st = &mut app.rest.oauth;
    st.job = Some(Arc::clone(&job));
    st.busy = true;
    st.status = "requesting token…".into();

    let token_url = st.token_url.clone();
    let mut args: Vec<String> = vec![
        "-s".into(),
        "-S".into(),
        "--max-time".into(),
        "30".into(),
        "-X".into(),
        "POST".into(),
        token_url,
        "-H".into(),
        "Content-Type: application/x-www-form-urlencoded".into(),
    ];
    if let Some(h) = header_auth {
        args.push("-H".into());
        args.push(h);
    }
    args.push("--data-raw".into());
    args.push(form);

    std::thread::spawn(move || {
        let t0 = Instant::now();
        let mut cmd = std::process::Command::new("curl.exe");
        cmd.args(&args);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        let res = cmd.output();
        let time_ms = t0.elapsed().as_millis();
        let mut slot = job.lock().unwrap();
        match res {
            Ok(out) => {
                let raw = String::from_utf8_lossy(&out.stdout).into_owned();
                let (status, _st, headers, body) = split_response(&raw);
                let err = if out.status.success() && status >= 200 && status < 300 {
                    None
                } else {
                    Some(format!("HTTP {status}: {}", body.chars().take(160).collect::<String>()))
                };
                *slot = Some(RestResult {
                    name: String::new(),
                    method: "POST".into(),
                    url: String::new(),
                    status,
                    status_text: _st,
                    time_ms,
                    size: body.len(),
                    headers,
                    body,
                    error: err,
                });
            }
            Err(e) => {
                *slot = Some(RestResult {
                    name: String::new(),
                    method: "POST".into(),
                    url: String::new(),
                    status: 0,
                    status_text: String::new(),
                    time_ms,
                    size: 0,
                    headers: String::new(),
                    body: String::new(),
                    error: Some(format!("curl.exe failed: {e}")),
                });
            }
        }
    });
}

impl App {
    /// Build + send the token request from the OAuth2 helper fields.
    pub fn oauth_get_token(&mut self) {
        if self.rest.oauth.token_url.trim().is_empty() || self.rest.oauth.client_id.trim().is_empty() {
            self.rest.oauth.status = "token URL and client id are required".into();
            return;
        }
        let st = &self.rest.oauth;
        let header_auth = if st.auth_in_header {
            Some(format!(
                "Authorization: Basic {}",
                b64(&format!("{}:{}", st.client_id, st.client_secret))
            ))
        } else {
            None
        };
        let mut form = String::new();
        match st.grant {
            0 => {
                form.push_str("grant_type=client_credentials");
                if !st.auth_in_header {
                    form.push_str(&format!("&client_id={}", urlencode_form(&st.client_id)));
                    form.push_str(&format!("&client_secret={}", urlencode_form(&st.client_secret)));
                }
            }
            1 => {
                form.push_str("grant_type=password");
                if !st.auth_in_header {
                    form.push_str(&format!("&client_id={}", urlencode_form(&st.client_id)));
                    form.push_str(&format!("&client_secret={}", urlencode_form(&st.client_secret)));
                }
                form.push_str(&format!("&username={}", urlencode_form(&st.username)));
                form.push_str(&format!("&password={}", urlencode_form(&st.password)));
            }
            _ => {
                form.push_str("grant_type=refresh_token");
                form.push_str(&format!("&refresh_token={}", urlencode_form(&st.refresh_token)));
                if !st.auth_in_header {
                    form.push_str(&format!("&client_id={}", urlencode_form(&st.client_id)));
                }
            }
        }
        if !st.scope.trim().is_empty() && st.grant != 2 {
            form.push_str(&format!("&scope={}", urlencode_form(&st.scope)));
        }
        oauth_send(self, form, header_auth);
    }

    pub fn oauth_poll(&mut self) {
        let done = self
            .rest
            .oauth
            .job
            .as_ref()
            .and_then(|j| j.lock().unwrap().take());
        if let Some(res) = done {
            self.rest.oauth.busy = false;
            if let Some(err) = res.error {
                self.rest.oauth.status = format!("✗ {err}");
                return;
            }
            match serde_json::from_str::<serde_json::Value>(res.body.trim()) {
                Ok(v) => {
                    let tok = v.get("access_token").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    if tok.is_empty() {
                        self.rest.oauth.status = "✗ no access_token in response".into();
                        return;
                    }
                    self.rest.oauth.token = tok;
                    self.rest.oauth.token_type = v
                        .get("token_type")
                        .and_then(|x| x.as_str())
                        .unwrap_or("Bearer")
                        .to_string();
                    self.rest.oauth.refresh_token = v
                        .get("refresh_token")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string();
                    let secs = v.get("expires_in").and_then(|x| x.as_i64()).unwrap_or(3600);
                    self.rest.oauth.expires_at =
                        Some(Instant::now() + std::time::Duration::from_secs(secs.saturating_sub(30) as u64));
                    self.rest.oauth.status = format!("✓ token saved (expires in {secs}s)");
                }
                Err(_) => {
                    self.rest.oauth.status = "✗ response is not JSON".into();
                }
            }
        }
    }
}

fn urlencode_form(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

// ---------------------------------------------------------------- UI

pub fn show(app: &mut App, ui: &mut egui::Ui, t: &Theme) {
    // poll finished job
    if app.rest.busy {
        if let Some(job) = &app.rest.job {
            let done = job.lock().unwrap().take();
            if let Some(mut r) = done {
                // merge request meta
                if let Some(req) = current_requests(app).get(app.rest.sel.min(999)) {
                    r.name = req.name.clone();
                    r.method = req.method.clone();
                    r.url = req.url.clone();
                }
                store_capture(app, &r);
                app.rest.results.push(r);
                if app.rest.results.len() > 20 {
                    app.rest.results.remove(0);
                }
                app.rest.busy = false;
            }
        }
    }
    // poll oauth job
    if app.rest.oauth.busy {
        app.oauth_poll();
    }

    let ext = app.active.map(|i| app.docs[i].ext()).unwrap_or_default();
    let is_http_doc = matches!(ext.as_str(), "http" | "rest");

    // refresh the env list (cheap file read, once per frame is fine here)
    let (env_names, env_file, _) = load_envs(app);
    app.rest.env_names = env_names;
    app.rest.env_file = env_file;
    if !app.rest.env_names.is_empty()
        && !app.rest.env_names.contains(&app.rest.env_sel)
        && !app.rest.env_sel.is_empty()
    {
        app.rest.env_sel.clear();
    }

    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("REST CLIENT")
                .size(10.5)
                .strong()
                .color(t.text_dim),
        );
        if is_http_doc {
            if let Some(reqs) = current_requests_opt(app) {
                let sel = app.rest.sel.min(reqs.len().saturating_sub(1));
                egui::ComboBox::from_id_source("rea_rest_sel")
                    .width(220.0)
                    .selected_text(if reqs.is_empty() {
                        "no requests".to_string()
                    } else {
                        reqs[sel].name.clone()
                    })
                    .show_ui(ui, |ui| {
                        for (ri, req) in reqs.iter().enumerate() {
                            ui.selectable_value(&mut app.rest.sel, ri, &req.name);
                        }
                    });
                let busy = app.rest.busy;
                if ui
                    .add_enabled(!busy, egui::Button::new(egui::RichText::new("▶ Send").size(11.5).strong()))
                    .clicked()
                {
                    send_current(app);
                }
                ui.label(
                    egui::RichText::new(format!("{} request(s)", reqs.len()))
                        .size(10.5)
                        .color(t.text_faint),
                );
            }
        } else {
            ui.label(
                egui::RichText::new("Open a .http file to define requests, e.g.  GET https://api.github.com")
                    .size(10.5)
                    .color(t.text_faint),
            );
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if app.rest.busy {
                ui.label(
                    egui::RichText::new("● sending…")
                        .size(10.5)
                        .color(t.warn),
                );
            }
            // ---- v1.7: environment picker ----
            if !app.rest.env_names.is_empty() {
                egui::ComboBox::from_id_source("rea_rest_env")
                    .width(90.0)
                    .selected_text(if app.rest.env_sel.is_empty() {
                        "No Env"
                    } else {
                        app.rest.env_sel.as_str()
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut app.rest.env_sel, String::new(), "No Env");
                        for n in app.rest.env_names.clone() {
                            ui.selectable_value(&mut app.rest.env_sel, n.clone(), n);
                        }
                    });
                let tip = app
                    .rest
                    .env_file
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                ui.label(
                    egui::RichText::new("🌐")
                        .size(11.0)
                        .color(t.text_dim),
                )
                .on_hover_text(format!(
                    "Environments from {tip}\n{{name}} resolves: env vars → @file vars\n{{oauth_token}} uses the OAuth2 helper token"
                ));
            }
            if ui
                .button(egui::RichText::new("Clear").size(10.5).color(t.text_dim))
                .clicked()
            {
                app.rest.results.clear();
            }
            ui.toggle_value(&mut app.rest.show_headers, egui::RichText::new("Headers").size(10.5).color(t.text_dim));
        });
    });
    ui.separator();

    if !is_http_doc {
        ui.add_space(6.0);
        ui.vertical_centered(|ui| {
            ui.add_space(20.0);
            ui.label(egui::RichText::new("⇅  REST Client").size(15.0).strong().color(t.text_dim));
            ui.label(
                egui::RichText::new(
                    "Create a file like api.http in your workspace:\n\n@host = https://api.github.com\n\n# @name users\n### users\nGET {{host}}/users/octocat\n\n### create\nPOST https://httpbin.org/post\nContent-Type: application/json\n\n{\"name\": \"ada\"}",
                )
                .size(12.0)
                .color(t.text_faint),
            );
            if ui.button(egui::RichText::new("＋ New api.http").size(11.5)).clicked() {
                app.open_untitled_http();
            }
        });
        oauth2_section(app, ui, t);
        return;
    }

    let Some(i) = app.active else { return };
    let text = app.docs[i].text.clone();
    let (reqs, vars) = parse_file(&text);
    if reqs.is_empty() {
        ui.label(egui::RichText::new("No requests found — start with:  GET https://…").size(11.5).color(t.text_faint));
        return;
    }
    let sel = app.rest.sel.min(reqs.len() - 1);
    let mut req = reqs[sel].clone();
    // show the RESOLVED url so users see what will actually be sent
    req.url = resolve_vars(app, &req.url, &vars);

    // captures hint
    if !app.rest.captures.is_empty() {
        let names: Vec<String> = app.rest.captures.keys().cloned().collect();
        ui.label(
            egui::RichText::new(format!(
                "captured: {}  (use {{{{name.body.$.path}}}} in later requests)",
                names.join(", ")
            ))
            .size(10.0)
            .color(t.ok),
        );
    }

    // response area
    let Some(res) = app.rest.results.last().cloned() else {
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(format!("Press ▶ Send to run  {} {}", req.method, req.url))
                .size(11.5)
                .color(t.text_faint),
        );
        oauth2_section(app, ui, t);
        return;
    };

    // status chip row
    ui.horizontal(|ui| {
        let (col, label) = if let Some(e) = &res.error {
            (t.err, format!("ERR {e}"))
        } else if (200..300).contains(&res.status) {
            (t.ok, format!("{} {}", res.status, res.status_text))
        } else if res.status >= 400 {
            (t.err, format!("{} {}", res.status, res.status_text))
        } else {
            (t.warn, format!("{} {}", res.status, res.status_text))
        };
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), egui::Sense::hover());
        ui.painter().rect_filled(r, Rounding::same(5.0), col.gamma_multiply(0.16));
        ui.painter().text(
            r.left_center() + vec2(8.0, 0.0),
            egui::Align2::LEFT_CENTER,
            format!("{} — {} {}", res.name, res.method, res.url),
            egui::FontId::proportional(11.0),
            t.text_dim,
        );
        ui.painter().text(
            r.right_center() - vec2(8.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            format!("{label}   {} ms   {} B", res.time_ms, res.size),
            egui::FontId::monospace(11.0),
            col,
        );
    });

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .stick_to_bottom(false)
        .show(ui, |ui| {
            if app.rest.show_headers && !res.headers.is_empty() {
                ui.add_space(4.0);
                ui.label(egui::RichText::new(&res.headers).size(11.0).color(t.text_faint).font(egui::FontId::monospace(11.0)));
                ui.separator();
            }
            ui.add_space(4.0);
            let body = if res.body.is_empty() {
                "(empty body)".to_string()
            } else {
                res.body.clone()
            };
            ui.add(
                egui::TextEdit::multiline(&mut body.as_str())
                    .font(egui::TextStyle::Monospace)
                    .desired_rows(10)
                    .desired_width(f32::INFINITY)
                    .frame(false)
                    .interactive(false),
            );
        });

    oauth2_section(app, ui, t);
}

/// v1.7: OAuth2 token helper (collapsible section at the bottom).
fn oauth2_section(app: &mut App, ui: &mut egui::Ui, t: &Theme) {
    ui.add_space(6.0);
    ui.separator();
    let head = if app.rest.oauth.token_valid() {
        egui::RichText::new("🔑 OAuth2 — token ready ({{oauth_token}})")
            .size(10.5)
            .strong()
            .color(t.ok)
    } else if app.rest.oauth.busy {
        egui::RichText::new("🔑 OAuth2 — requesting…")
            .size(10.5)
            .strong()
            .color(t.warn)
    } else {
        egui::RichText::new("🔑 OAuth2")
            .size(10.5)
            .strong()
            .color(t.text_dim)
    };
    egui::CollapsingHeader::new(head)
        .default_open(app.rest.oauth.open)
        .show(ui, |ui| {
            app.rest.oauth.open = true;
            let grants = ["client_credentials", "password", "refresh_token"];
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("grant:").size(10.5).color(t.text_dim));
                for (gi, g) in grants.iter().enumerate() {
                    if ui
                        .selectable_label(app.rest.oauth.grant == gi, egui::RichText::new(*g).size(10.5))
                        .clicked()
                    {
                        app.rest.oauth.grant = gi;
                    }
                }
            });
            let w = ui.available_width() - 110.0;
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("token URL:").size(10.5).color(t.text_dim));
                ui.add(
                    egui::TextEdit::singleline(&mut app.rest.oauth.token_url)
                        .hint_text("https://auth.example.com/oauth2/token")
                        .desired_width(w),
                );
            });
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("client id:").size(10.5).color(t.text_dim));
                ui.add(egui::TextEdit::singleline(&mut app.rest.oauth.client_id).desired_width(w * 0.5));
                ui.label(egui::RichText::new("secret:").size(10.5).color(t.text_dim));
                ui.add(
                    egui::TextEdit::singleline(&mut app.rest.oauth.client_secret)
                        .password(true)
                        .desired_width(w * 0.42),
                );
            });
            if app.rest.oauth.grant == 1 {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("username:").size(10.5).color(t.text_dim));
                    ui.add(egui::TextEdit::singleline(&mut app.rest.oauth.username).desired_width(w * 0.45));
                    ui.label(egui::RichText::new("password:").size(10.5).color(t.text_dim));
                    ui.add(
                        egui::TextEdit::singleline(&mut app.rest.oauth.password)
                            .password(true)
                            .desired_width(w * 0.45),
                    );
                });
            }
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("scope:").size(10.5).color(t.text_dim));
                ui.add(egui::TextEdit::singleline(&mut app.rest.oauth.scope).desired_width(w * 0.6));
                ui.checkbox(&mut app.rest.oauth.auth_in_header, egui::RichText::new("Basic header").size(10.5));
            });
            ui.horizontal(|ui| {
                let busy = app.rest.oauth.busy;
                if ui
                    .add_enabled(!busy, egui::Button::new(egui::RichText::new("🔑 Get Token").size(11.0).strong()))
                    .on_hover_text("POSTs the grant to the token URL and caches the result")
                    .clicked()
                {
                    app.oauth_get_token();
                }
                if !app.rest.oauth.refresh_token.is_empty() && app.rest.oauth.grant != 2 {
                    if ui.button(egui::RichText::new("↻ Refresh").size(11.0)).clicked() {
                        app.rest.oauth.grant = 2;
                        app.oauth_get_token();
                        app.rest.oauth.grant = 0;
                    }
                }
                if !app.rest.oauth.token.is_empty() {
                    let state = if app.rest.oauth.token_valid() { "✓ valid" } else { "expired" };
                    ui.label(
                        egui::RichText::new(format!("{} — use {{oauth_token}} in headers", state))
                            .size(10.5)
                            .color(if app.rest.oauth.token_valid() { t.ok } else { t.warn }),
                    );
                }
            });
            if !app.rest.oauth.status.is_empty() {
                ui.label(
                    egui::RichText::new(app.rest.oauth.status.clone())
                        .size(10.0)
                        .color(t.text_faint),
                );
            }
        });
}

/// Current .http requests (cached parse of the active doc).
fn current_requests_opt(app: &App) -> Option<Vec<HttpReq>> {
    let i = app.active?;
    let ext = app.docs[i].ext();
    if !matches!(ext.as_str(), "http" | "rest") {
        return None;
    }
    Some(parse_requests(&app.docs[i].text))
}

fn current_requests(app: &App) -> Vec<HttpReq> {
    current_requests_opt(app).unwrap_or_default()
}

/// Send the selected request (palette / button / Alt+R).
pub fn send_current(app: &mut App) {
    let reqs = current_requests(app);
    if reqs.is_empty() {
        app.set_status("REST Client: no requests in the active .http file");
        return;
    }
    let sel = app.rest.sel.min(reqs.len() - 1);
    let req = reqs[sel].clone();
    send(app, &req);
    app.set_status(format!("REST Client: sending {} {}…", req.method, req.url));
}

impl App {
    pub fn open_untitled_http(&mut self) {
        self.untitled_counter += 1;
        let name = format!("api-{}.http", self.untitled_counter);
        let text = "@host = https://api.github.com\n\n# @name users\n### users\nGET {{host}}/users/octocat\n\n### create\nPOST https://httpbin.org/post\nContent-Type: application/json\n\n{\"name\": \"ada\"}\n".to_string();
        self.docs.push(crate::app::Doc::new(None, name, text));
        self.active = Some(self.docs.len() - 1);
    }
}

// keep Color32 referenced (used via t only)
#[allow(dead_code)]
fn _c() -> Color32 {
    Color32::WHITE
}
