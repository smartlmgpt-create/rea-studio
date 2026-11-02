//! PLUGIN: Live Server (v1.7)
//!
//! A tiny dependency-free HTTP(S) server (std::net + SChannel via native-tls)
//! that serves the workspace folder and injects an auto-reload probe into
//! every HTML response. v1.7:
//!  * ALL files under the root are watched (any extension — images, fonts,
//!    data files…) — any change reloads the browser, not just html/css/js
//!  * optional HTTPS with a self-signed certificate (generated once with
//!    PowerShell's New-SelfSignedCertificate, served through SChannel)
//!  * one thread per connection (a slow client no longer blocks the loop)
//!  * v1.7.5 — PHP support: `.php` requests run through `php-cgi` (CGI/1.1)
//!    when it is available (next to rea-studio.exe or on PATH); a friendly
//!    setup guide page is served otherwise. Full request bodies (POST) are
//!    now read (previously only the first 4096 bytes were seen at all).
//!  * v1.7.6 — cross-platform builds (Linux + macOS): php-cgi is also probed
//!    in the usual unix install locations (GUI-launched apps get a minimal
//!    PATH on macOS), and the setup-guide page shows OS-specific steps.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::theme::Theme;

const RELOAD_JS: &str = r#"<script data-rea-live>(function(){
var s=null; setInterval(function(){
 fetch('/__rea__/stamp',{cache:'no-store'}).then(function(r){return r.text()})
 .then(function(t){ if(s===null){s=t;} else if(s!==t){ location.reload(); } })
 .catch(function(){});},700);})();</script>"#;

pub struct LiveServer {
    pub running: bool,
    pub port: u16,
    pub https_port: u16,
    pub https_on: bool,
    pub root: PathBuf,
    stop: Arc<AtomicBool>,
    /// served-requests counter (read from the accept thread)
    served: Arc<Mutex<u32>>,
    /// TLS acceptor when HTTPS is enabled (SChannel via native-tls, Windows)
    #[cfg(windows)]
    tls: Option<Arc<native_tls::TlsAcceptor>>,
    /// TLS server config when HTTPS is enabled (rustls, unix — v1.8)
    #[cfg(unix)]
    tls: Option<Arc<rustls::ServerConfig>>,
    /// last error reported by the thread (bind failure, …)
    pub error: Option<String>,
}

impl Default for LiveServer {
    fn default() -> Self {
        Self {
            running: false,
            port: 5500,
            https_port: 5540,
            https_on: false,
            root: PathBuf::new(),
            stop: Arc::new(AtomicBool::new(false)),
            served: Arc::new(Mutex::new(0)),
            #[cfg(windows)]
            tls: None,
            #[cfg(unix)]
            tls: None,
            error: None,
        }
    }
}

impl LiveServer {
    /// Start serving `root`. Picks the first free port in 5500..=5519
    /// (HTTP) and 5540..=5549 (HTTPS when enabled).
    pub fn start(&mut self, root: PathBuf, https: bool, https_port: u16, cfg_dir: &Path) -> Result<u16, String> {
        if self.running {
            return Ok(self.port);
        }
        let mut bound = None;
        let mut last_err = String::new();
        for port in 5500..=5519 {
            match TcpListener::bind(("127.0.0.1", port)) {
                Ok(l) => {
                    bound = Some((l, port));
                    break;
                }
                Err(e) => last_err = e.to_string(),
            }
        }
        let (listener, port) = bound.ok_or_else(|| format!("no free port: {last_err}"))?;

        self.stop = Arc::new(AtomicBool::new(false));
        self.served = Arc::new(Mutex::new(0));
        #[cfg(windows)]
        {
            self.tls = None;
        }
        let stop = Arc::clone(&self.stop);
        let served = Arc::clone(&self.served);

        // ---- optional HTTPS acceptor ----
        // Windows: SChannel via native-tls (self-signed PFX in memory)
        // unix:    rustls + rcgen, pure Rust — no OpenSSL (v1.8)
        #[cfg(windows)]
        let tls_on: bool = if https {
            match ensure_https_cert(cfg_dir) {
                Ok(p12) => match native_tls::Identity::from_pkcs12(&p12, "rea-live") {
                    Ok(id) => match native_tls::TlsAcceptor::builder(id).build() {
                        Ok(a) => {
                            self.tls = Some(Arc::new(a));
                            true
                        }
                        Err(e) => return Err(format!("TLS acceptor failed: {e}")),
                    },
                    Err(e) => return Err(format!("certificate load failed: {e}")),
                },
                Err(e) => return Err(format!("HTTPS certificate failed: {e}")),
            }
        } else {
            false
        };
        #[cfg(unix)]
        let tls_on: bool = if https {
            match ensure_https_config() {
                Ok(cfg) => {
                    self.tls = Some(Arc::new(cfg));
                    true
                }
                Err(e) => return Err(format!("HTTPS setup failed: {e}")),
            }
        } else {
            false
        };
        let _ = https_port;

        listener.set_nonblocking(true).ok();
        let tls2 = self.tls.clone();
        std::thread::spawn(move || {
            loop {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                match listener.accept() {
                    Ok((stream, _)) => {
                        *served.lock().unwrap() += 1;
                        // guard against clients that never finish their request
                        stream
                            .set_read_timeout(Some(std::time::Duration::from_secs(15)))
                            .ok();
                        stream
                            .set_write_timeout(Some(std::time::Duration::from_secs(30)))
                            .ok();
                        let tls = tls2.clone();
                        std::thread::spawn(move || {
                            match tls {
                                #[cfg(windows)]
                                Some(acc) => match acc.accept(stream) {
                                    Ok(s) => handle_conn(s, true),
                                    Err(_) => {}
                                },
                                #[cfg(unix)]
                                Some(cfg) => {
                                    match rustls::ServerConnection::new(cfg) {
                                        Ok(conn) => {
                                            let ts = rustls::StreamOwned::new(conn, stream);
                                            handle_conn(ts, true);
                                        }
                                        Err(_) => {}
                                    }
                                }
                                #[cfg(any(windows, unix))]
                                None => handle_conn(stream, false),
                            }
                        });
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(60));
                    }
                    Err(_) => std::thread::sleep(std::time::Duration::from_millis(200)),
                }
            }
        });

        PORT.store(port, Ordering::Relaxed);
        self.running = true;
        self.port = port;
        self.https_on = tls_on;
        self.https_port = if tls_on { port } else { https_port };
        self.root = root;
        self.error = None;
        Ok(port)
    }

    pub fn stop(&mut self) {
        if self.running {
            self.stop.store(true, Ordering::Relaxed);
            self.running = false;
            self.https_on = false;
        }
    }

    pub fn served(&self) -> u32 {
        *self.served.lock().unwrap()
    }

    pub fn url(&self) -> String {
        if self.https_on {
            format!("https://127.0.0.1:{}", self.port)
        } else {
            format!("http://127.0.0.1:{}", self.port)
        }
    }
}

impl Drop for LiveServer {
    fn drop(&mut self) {
        self.stop();
    }
}

// ---------------------------------------------------------------- HTTPS cert

/// Generate (once) a self-signed PKCS#12 certificate via PowerShell and
/// return its bytes. SECURITY: the PFX is exported IN MEMORY (base64 on
/// stdout) — the old code wrote `rea-live.p12` to disk protected by the
/// hard-coded password "rea-live", i.e. an unprotected private key file.
/// The cert itself lives in the per-user certificate store (ACL-protected).
#[cfg(windows)]
fn ensure_https_cert(_cfg_dir: &Path) -> Result<Vec<u8>, String> {
    let ps = "\
        $ErrorActionPreference='Stop'; \
        $c = Get-ChildItem Cert:\\CurrentUser\\My | \
             Where-Object { $_.FriendlyName -eq 'REA Live Server' -and $_.NotAfter -gt (Get-Date) } | \
             Select-Object -First 1; \
        if (-not $c) { \
            $c = New-SelfSignedCertificate -DnsName 'localhost' -FriendlyName 'REA Live Server' \
                 -CertStoreLocation Cert:\\CurrentUser\\My -NotAfter (Get-Date).AddYears(5); \
        }; \
        $p = ConvertTo-SecureString -String 'rea-live' -Force -AsPlainText; \
        [Convert]::ToBase64String($c.Export('Pfx', $p))";
    let out = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-NonInteractive", "-Command", ps])
        .output()
        .map_err(|e| format!("powershell: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    use base64::Engine;
    let b64 = String::from_utf8_lossy(&out.stdout).trim().to_string();
    base64::engine::general_purpose::STANDARD
        .decode(b64.as_bytes())
        .map_err(|e| format!("cert decode: {e}"))
}

#[cfg(not(windows))]
fn ensure_https_cert(_cfg_dir: &Path) -> Result<Vec<u8>, String> {
    Err("internal error: windows cert path on unix".into())
}

/// v1.8: build a rustls ServerConfig with a fresh self-signed certificate
/// (rcgen — pure Rust, no OpenSSL, no PowerShell). SANs cover both the
/// name and the literal IP the browser opens, to keep warnings minimal.
#[cfg(unix)]
fn ensure_https_config() -> Result<rustls::ServerConfig, String> {
    let ck = rcgen::generate_simple_self_signed(vec![
        "localhost".to_string(),
        "127.0.0.1".to_string(),
    ])
    .map_err(|e| format!("certificate generation: {e}"))?;
    let key = rustls::pki_types::PrivateKeyDer::Pkcs8(ck.key_pair.serialize_der().into());
    rustls::ServerConfig::builder_with_provider(rustls::crypto::ring::default_provider().into())
        .with_safe_default_protocol_versions()
        .map_err(|e| format!("TLS versions: {e}"))?
        .with_no_client_auth()
        .with_single_cert(vec![ck.cert.der().clone()], key)
        .map_err(|e| format!("TLS certificate: {e}"))
}

// ---------------------------------------------------------------- engine

fn handle_conn<S: Read + Write>(mut stream: S, https: bool) {
    let (head, body) = match read_request(&mut stream) {
        Ok(r) => r,
        Err(_) => return,
    };
    let first = head.lines().next().unwrap_or("").to_string();
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let raw_target = parts.next().unwrap_or("/").to_string();

    let (status, out_body, ct) = handle_request(&method, &raw_target, &head, &body, https);

    let head_out = format!(
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        status,
        ct,
        out_body.len()
    );
    let _ = stream.write_all(head_out.as_bytes());
    let _ = stream.write_all(&out_body);
    let _ = stream.flush();
}

/// Read one full HTTP request: head (request line + headers) plus an
/// optional body announced by Content-Length (POST forms / JSON — needed
/// for PHP). The old code read a single 4096-byte chunk and never saw
/// request bodies at all.
fn read_request<S: Read>(stream: &mut S) -> Result<(String, Vec<u8>), ()> {
    let mut buf: Vec<u8> = Vec::with_capacity(8192);
    let mut tmp = [0u8; 4096];
    let head_end = loop {
        if let Some(p) = find_subslice(&buf, b"\r\n\r\n") {
            break p;
        }
        if buf.len() > 64 * 1024 {
            return Err(()); // header cap
        }
        let n = stream.read(&mut tmp).map_err(|_| ())?;
        if n == 0 {
            return Err(()); // peer closed mid-request
        }
        buf.extend_from_slice(&tmp[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut clen = 0usize;
    for line in head.lines() {
        if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            clen = v.trim().parse().unwrap_or(0);
        }
    }
    const BODY_CAP: usize = 8 * 1024 * 1024; // 8 MB
    if clen > BODY_CAP {
        return Err(());
    }
    let mut body: Vec<u8> = buf[head_end + 4..].to_vec();
    while body.len() < clen {
        let n = stream.read(&mut tmp).map_err(|_| ())?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    body.truncate(clen);
    Ok((head, body))
}

/// (status, body, content-type) for one request.
fn handle_request(
    method: &str,
    raw_target: &str,
    head: &str,
    body: &[u8],
    https: bool,
) -> (String, Vec<u8>, String) {
    let (path_raw, query) = match raw_target.split_once('?') {
        Some((p, q)) => (p, q),
        None => (raw_target, ""),
    };
    let path = percent_decode(path_raw);

    if path == "/__rea__/stamp" {
        return (
            "200 OK".into(),
            dir_stamp().unwrap_or_else(|| "0".into()).into_bytes(),
            "text/plain".into(),
        );
    }

    let Some(root) = root_path() else {
        return ("404 Not Found".into(), Vec::new(), "text/html".into());
    };

    // resolve the target with the same traversal safety as before:
    // reject ANY parent-dir traversal (`..`, and percent-decoded `..`) —
    // components are checked BEFORE joining, because `Path::starts_with`
    // alone happily accepts `root/../../etc/passwd`.
    let rel = path.trim_start_matches('/');
    let target: PathBuf = if rel.is_empty() {
        let idx = root.join("index.html");
        if idx.exists() {
            idx
        } else if root.join("index.php").exists() {
            root.join("index.php")
        } else {
            return (
                "200 OK".into(),
                dir_listing(&root).into_bytes(),
                "text/html".into(),
            );
        }
    } else {
        let rel_p = Path::new(rel);
        let bad = rel_p
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir | std::path::Component::RootDir));
        let p = root.join(rel_p);
        if bad || !p.starts_with(&root) {
            return ("404 Not Found".into(), Vec::new(), "text/html".into());
        }
        p
    };

    // ---- v1.7.5: PHP through php-cgi (CGI/1.1) ----
    let is_php = target
        .extension()
        .map(|e| e.to_string_lossy().eq_ignore_ascii_case("php"))
        .unwrap_or(false);
    if is_php {
        return run_php(&target, &method, &path, query, head, body, &root, https);
    }

    match std::fs::read(&target) {
        Ok(bytes) => {
            let mime = mime_of(&target);
            if mime == "text/html" {
                // inject the reload probe WITHOUT corrupting the page
                // (legacy codepages / UTF-16 — see html_with_probe)
                let (b, ct) = html_with_probe(bytes);
                ("200 OK".into(), b, ct)
            } else {
                // non-HTML: pass the raw bytes through untouched
                ("200 OK".into(), bytes, mime)
            }
        }
        Err(_) => ("404 Not Found".into(), Vec::new(), "text/html".into()),
    }
}

/// v1.7.6: locate `php-cgi` on any platform. Windows: CreateProcess already
/// searches the rea-studio.exe folder + PATH. macOS/Linux: a GUI-launched app
/// inherits a minimal PATH (macOS: /usr/bin:/bin:/usr/sbin:/sbin), so the
/// common install locations are probed explicitly. Every candidate is tested
/// with `php-cgi -v` so a stale/broken entry never wins.
fn find_php_cgi() -> Option<Command> {
    let mut cands: Vec<PathBuf> = Vec::new();
    #[cfg(windows)]
    if let Ok(exe) = std::env::current_exe() {
        if let Some(d) = exe.parent() {
            cands.push(d.join("php-cgi.exe"));
        }
    }
    #[cfg(not(windows))]
    for p in [
        "/usr/bin/php-cgi",
        "/usr/local/bin/php-cgi",
        "/opt/homebrew/bin/php-cgi", // Homebrew on Apple Silicon
        "/opt/local/bin/php-cgi",    // MacPorts
    ] {
        cands.push(PathBuf::from(p));
    }
    // bare name → PATH lookup (CreateProcess on Windows, execvp on unix)
    cands.push(PathBuf::from("php-cgi"));
    for c in cands {
        let ok = Command::new(&c)
            .arg("-v")
            .stdin(Stdio::null())
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if ok {
            return Some(Command::new(c));
        }
    }
    None
}

/// Run a `.php` file through `php-cgi` (CGI/1.1). The lookup is automatic
/// (see `find_php_cgi`).
#[allow(clippy::too_many_arguments)]
fn run_php(
    script: &Path,
    method: &str,
    script_name: &str,
    query: &str,
    head: &str,
    body: &[u8],
    root: &Path,
    https: bool,
) -> (String, Vec<u8>, String) {
    let mut cmd = match find_php_cgi() {
        Some(c) => c,
        None => return php_missing_page("php-cgi was not found on this system"),
    };
    cmd.current_dir(root);

    // forward a few request headers as CGI environment variables
    let mut ctype = String::new();
    for line in head.lines().skip(1) {
        if let Some((k, v)) = line.split_once(':') {
            let kl = k.trim().to_ascii_lowercase();
            let v = v.trim();
            match kl.as_str() {
                "content-type" => ctype = v.to_string(),
                "cookie" => {
                    cmd.env("HTTP_COOKIE", v);
                }
                "user-agent" => {
                    cmd.env("HTTP_USER_AGENT", v);
                }
                "accept" => {
                    cmd.env("HTTP_ACCEPT", v);
                }
                "accept-language" => {
                    cmd.env("HTTP_ACCEPT_LANGUAGE", v);
                }
                _ => {}
            }
        }
    }

    cmd.env("REDIRECT_STATUS", "200"); // php-cgi's force_redirect guard
    cmd.env("GATEWAY_INTERFACE", "CGI/1.1");
    cmd.env("SERVER_PROTOCOL", "HTTP/1.1");
    cmd.env(
        "SERVER_SOFTWARE",
        format!("REA-Live/{}", env!("CARGO_PKG_VERSION")),
    );
    cmd.env("SERVER_NAME", "127.0.0.1");
    cmd.env("SERVER_PORT", PORT.load(Ordering::Relaxed).to_string());
    cmd.env("REMOTE_ADDR", "127.0.0.1");
    cmd.env("REQUEST_METHOD", method);
    cmd.env(
        "REQUEST_URI",
        if query.is_empty() {
            script_name.to_string()
        } else {
            format!("{script_name}?{query}")
        },
    );
    cmd.env("SCRIPT_NAME", script_name);
    cmd.env("SCRIPT_FILENAME", script);
    cmd.env("DOCUMENT_ROOT", root);
    if !query.is_empty() {
        cmd.env("QUERY_STRING", query);
    }
    if !ctype.is_empty() {
        cmd.env("CONTENT_TYPE", &ctype);
    }
    cmd.env("CONTENT_LENGTH", body.len().to_string());
    if https {
        cmd.env("HTTPS", "on");
    }

    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return php_missing_page(&e.to_string()),
    };
    if let Some(mut si) = child.stdin.take() {
        let _ = si.write_all(body); // dropped afterwards → EOF for the script
    }
    let out = match child.wait_with_output() {
        Ok(o) => o,
        Err(e) => {
            return (
                "500 Internal Server Error".into(),
                format!("php-cgi: {e}").into_bytes(),
                "text/plain".into(),
            )
        }
    };
    parse_cgi_response(&out.stdout)
}

/// Parse a CGI/1.1 response: header block, blank line, then the body.
/// `Status:` overrides the HTTP status, `Content-Type:` the MIME type.
fn parse_cgi_response(raw: &[u8]) -> (String, Vec<u8>, String) {
    let mut status = "200 OK".to_string();
    let mut ctype = "text/html".to_string();
    let (head, body) = match find_subslice(raw, b"\r\n\r\n") {
        Some(p) => (&raw[..p], &raw[p + 4..]),
        None => match find_subslice(raw, b"\n\n") {
            Some(p) => (&raw[..p], &raw[p + 2..]),
            None => (&raw[..0], raw),
        },
    };
    for line in String::from_utf8_lossy(head).lines() {
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("status:") {
            if let Some((_, v)) = line.split_once(':') {
                let s = v.trim();
                if !s.is_empty() {
                    status = s.to_string();
                }
            }
        } else if lower.starts_with("content-type:") {
            if let Some((_, v)) = line.split_once(':') {
                let s = v.trim();
                if !s.is_empty() {
                    ctype = s.to_string();
                }
            }
        }
    }
    // some php-cgi builds emit a bare code ("Status: 422") — add a reason
    // phrase so the HTTP status line stays well-formed for every client
    if !status.contains(' ') {
        let reason = match status.as_str() {
            "100" => "Continue",
            "200" => "OK",
            "201" => "Created",
            "204" => "No Content",
            "301" => "Moved Permanently",
            "302" => "Found",
            "304" => "Not Modified",
            "400" => "Bad Request",
            "401" => "Unauthorized",
            "403" => "Forbidden",
            "404" => "Not Found",
            "405" => "Method Not Allowed",
            "409" => "Conflict",
            "413" => "Payload Too Large",
            "422" => "Unprocessable Entity",
            "429" => "Too Many Requests",
            "500" => "Internal Server Error",
            "501" => "Not Implemented",
            "502" => "Bad Gateway",
            "503" => "Service Unavailable",
            _ => "Response",
        };
        status = format!("{status} {reason}");
    }
    (status, body.to_vec(), ctype)
}

/// Friendly page shown when a `.php` file is requested but php-cgi is not
/// found. The steps shown are OS-specific (v1.7.6).
fn php_missing_page(err: &str) -> (String, Vec<u8>, String) {
    let (step1, step2) = if cfg!(windows) {
        (
            "PHP را از <code>windows.php.net/download</code> بگیرید (نسخهٔ Thread Safe، فایل زیپ) — یا اگر XAMPP دارید، همین الان کافی است: <code>C:\\xampp\\php</code>",
            "پوشهٔ PHP را به PATH ویندوز اضافه کنید — یا ساده‌تر: کل پوشهٔ PHP را کنار <code>rea-studio.exe</code> کپی کنید",
        )
    } else if cfg!(target_os = "macos") {
        (
            "مک: در ترمینال بزنید <code>brew install php</code> — باینری برنامه مسیرهای Homebrew (<code>/opt/homebrew/bin</code> و <code>/usr/local/bin</code>) را هم جست‌وجو می‌کند",
            "اگر Homebrew ندارید، از <code>brew.sh</code> نصبش کنید؛ بعد Live Server را خاموش و دوباره روشن کنید",
        )
    } else {
        (
            "لینوکس: در ترمینال بزنید <code>sudo apt install php-cgi</code> (دبیان/اوبونتو) یا <code>sudo dnf install php</code> (فدورا/ردهت)",
            "بستهٔ php-cgi معمولاً در مخازن رسمی توزیع شما هست؛ بعد از نصب Live Server را خاموش و دوباره روشن کنید",
        )
    };
    let css = "body{font-family:'Segoe UI',Tahoma,sans-serif;background:#14161f;color:#e8e9f0;\
max-width:720px;margin:48px auto;padding:0 20px;line-height:2}\
code{background:#232636;padding:2px 8px;border-radius:6px;color:#7ee2a8;direction:ltr;display:inline-block}\
ol{padding-right:20px}";
    let html = format!(
        "<!DOCTYPE html><html lang=\"fa\" dir=\"rtl\"><head><meta charset=\"utf-8\">\
<title>REA Live Server — PHP</title><style>{css}</style></head><body>\
<h2>🐘 پی‌اچ‌پی هنوز فعال نیست — php-cgi پیدا نشد</h2>\
<p>سرور زندهٔ REA فایل‌های <code>.php</code> را اجرا می‌کند، اما <code>php-cgi</code> روی سیستم پیدا نشد.</p>\
<h3>راه‌اندازی در ۳ قدم</h3>\
<ol>\
<li>{step1}</li>\
<li>{step2}</li>\
<li>Live Server را خاموش و دوباره روشن کنید</li>\
</ol>\
<p>برای آزمایش، فایل <code>php/health.php</code> سایت نمونه را در مرورگر باز کنید.</p>\
<p style=\"color:#8b90a5\">جزئیات فنی: {err}</p>\
</body></html>"
    );
    (
        "200 OK".into(),
        html.into_bytes(),
        "text/html; charset=utf-8".into(),
    )
}

fn find_subslice(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

fn dir_listing(root: &Path) -> String {
    let mut rows = String::new();
    let mut entries: Vec<_> = std::fs::read_dir(root)
        .map(|rd| rd.filter_map(|e| e.ok()).collect::<Vec<_>>())
        .unwrap_or_default();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let dir = e.path().is_dir();
        let href = if dir {
            format!("{}/", name)
        } else {
            name.clone()
        };
        rows.push_str(&format!(
            "<li><a href=\"{}\">{}</a></li>",
            href,
            html_esc(&name)
        ));
    }
    format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>Live Server</title>\
         <style>body{{font-family:system-ui;max-width:640px;margin:40px auto;padding:0 16px}}\
         li{{margin:4px 0}}</style></head><body>\
         <h2>Live Server — {}</h2><ul>{}</ul></body></html>",
        html_esc(&root.display().to_string()),
        rows
    )
}

fn html_esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn mime_of(p: &Path) -> String {
    match p
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .as_deref()
    {
        Some("html") | Some("htm") => "text/html",
        Some("css") => "text/css",
        Some("js") | Some("mjs") => "application/javascript",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("avif") => "image/avif",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        Some("txt") | Some("md") => "text/plain",
        Some("pdf") => "application/pdf",
        Some("xml") => "application/xml",
        Some("wasm") => "application/wasm",
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("ogg") => "audio/ogg",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        Some("csv") => "text/csv",
        Some("zip") => "application/zip",
        Some("map") => "application/json",
        _ => "application/octet-stream",
    }
    .to_string()
}

// ---- v1.7.3: charset-safe HTML serving ------------------------------------
// The probe used to be injected after a from_utf8_lossy round-trip, which
// destroyed legacy-encoded documents (windows-1256 Persian/Arabic pages,
// windows-1252, UTF-16, …). Now:
//   • BOM decides: UTF-16LE/BE is decoded to UTF-8 and served as UTF-8
//   • valid UTF-8 → old path (Content-Type gains charset=utf-8)
//   • anything else → the pure-ASCII probe is spliced into the RAW bytes and
//     the page's own <meta charset> is declared (default windows-1252); the
//     text is never re-encoded, so nothing can be mangled.

fn html_with_probe(bytes: Vec<u8>) -> (Vec<u8>, String) {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        // UTF-8 BOM — the BOM char passes through untouched
        return (inject_utf8(&bytes), "text/html; charset=utf-8".into());
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let dec = utf16_to_utf8(&bytes[2..], false);
        return (inject_str(dec), "text/html; charset=utf-8".into());
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let dec = utf16_to_utf8(&bytes[2..], true);
        return (inject_str(dec), "text/html; charset=utf-8".into());
    }
    match String::from_utf8(bytes) {
        Ok(s) => (inject_str(s), "text/html; charset=utf-8".into()),
        Err(e) => {
            let raw = e.into_bytes();
            let cs = sniff_charset(&raw).unwrap_or_else(|| "windows-1252".into());
            let script = RELOAD_JS.as_bytes();
            let pos = find_ascii_ci(&raw, b"</body>").unwrap_or(raw.len());
            let mut body = Vec::with_capacity(raw.len() + script.len());
            body.extend_from_slice(&raw[..pos]);
            body.extend_from_slice(script);
            body.extend_from_slice(&raw[pos..]);
            (body, format!("text/html; charset={cs}"))
        }
    }
}

/// Insert the probe into a known-UTF-8 document (string level, old behavior).
fn inject_str(mut s: String) -> Vec<u8> {
    if let Some(i) = s.rfind("</body>") {
        s.insert_str(i, RELOAD_JS);
    } else {
        s.push_str(RELOAD_JS);
    }
    s.into_bytes()
}

/// Insert the probe into bytes that are (almost certainly) UTF-8 but only
/// available as a slice; lossy decoding is a no-op for valid UTF-8.
fn inject_utf8(bytes: &[u8]) -> Vec<u8> {
    inject_str(String::from_utf8_lossy(bytes).into_owned())
}

/// Decode UTF-16 (BOM already stripped) to a Rust String (UTF-8).
fn utf16_to_utf8(b: &[u8], be: bool) -> String {
    let units = b.chunks_exact(2).map(|c| {
        if be {
            u16::from_be_bytes([c[0], c[1]])
        } else {
            u16::from_le_bytes([c[0], c[1]])
        }
    });
    char::decode_utf16(units)
        .map(|r| r.unwrap_or('\u{FFFD}'))
        .collect()
}

/// Case-insensitive ASCII search, LAST occurrence (mirrors the old rfind).
fn find_ascii_ci(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    let mut found = None;
    for i in 0..=hay.len() - needle.len() {
        if hay[i..i + needle.len()].eq_ignore_ascii_case(needle) {
            found = Some(i);
        }
    }
    found
}

/// First `charset=…` declaration in the first 2 KB (meta charset / http-equiv).
fn sniff_charset(b: &[u8]) -> Option<String> {
    let mut low = b[..b.len().min(2048)].to_vec();
    low.make_ascii_lowercase();
    let s = String::from_utf8_lossy(&low);
    let p = s.find("charset=")?;
    let rest = &s[p + "charset=".len()..];
    let rest = rest
        .strip_prefix('"')
        .or_else(|| rest.strip_prefix('\''))
        .unwrap_or(rest);
    let val: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if val.is_empty() {
        None
    } else {
        Some(val)
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("");
            if let Ok(v) = u8::from_str_radix(hex, 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        if b[i] == b'+' {
            out.push(b' ');
        } else {
            out.push(b[i]);
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ---- root shared with the thread via a global (set on start) ----
static ROOT: Mutex<Option<PathBuf>> = Mutex::new(None);
/// server port shared with the CGI environment (SERVER_PORT)
static PORT: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(5500);

pub fn set_root(r: PathBuf) {
    *ROOT.lock().unwrap() = Some(r);
}

fn root_path() -> Option<PathBuf> {
    ROOT.lock().unwrap().clone()
}

/// Change stamp of the served root: max mtime (secs) + file count over ALL
/// files (any extension; depth ≤ 4, ≤ 3000 entries). Browser reloads on
/// change — html, css, js, images, fonts, everything.
pub fn dir_stamp() -> Option<String> {
    let root = root_path()?;
    let t0 = Instant::now();
    let mut latest: u64 = 0;
    let mut count: u32 = 0;
    let mut stack = vec![(root, 0u8)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > 4 || count > 3000 || t0.elapsed().as_millis() > 250 {
            continue;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.filter_map(|e| e.ok()) {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_lowercase();
            if p.is_dir() {
                if !name.starts_with('.') && name != "node_modules" && name != "target" {
                    stack.push((p, depth + 1));
                }
            } else {
                count += 1;
                if let Ok(m) = e.metadata() {
                    if let Ok(mt) = m.modified() {
                        if let Ok(d) = mt.duration_since(std::time::UNIX_EPOCH) {
                            latest = latest.max(d.as_secs());
                        }
                    }
                }
            }
        }
    }
    Some(format!("{latest}-{count}"))
}

// ---------------------------------------------------------------- UI pill

/// Small clickable pill for the status bar / panels: shows state and
/// start/stops the server.
pub fn pill_ui(
    app: &mut crate::app::App,
    ui: &mut egui::Ui,
    t: &Theme,
    accent_text: egui::Color32,
) {
    let running = app.live.running;
    let label = if running {
        let proto = if app.live.https_on { "https" } else { "http" };
        format!("● Live :{} ({})", app.live.port, proto)
    } else {
        "○ Live Server".to_string()
    };
    let resp = ui.add(
        egui::Button::new(
            egui::RichText::new(label)
                .size(10.5)
                .color(if running { accent_text } else { t.text_dim }),
        )
        .fill(egui::Color32::TRANSPARENT)
        .stroke(egui::Stroke::NONE),
    );
    let clicked = resp.clicked();
    resp.on_hover_text(if running {
        "Live Server running — click to stop"
    } else {
        "Start Live Server for this workspace"
    });
    if clicked {
        if running {
            app.live_stop();
        } else {
            app.live_start();
        }
    }
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn server_http_and_https_roundtrip() {
        let dir = std::env::temp_dir().join("rea-srvtest-180");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("index.html"),
            b"<html><body>REA_HTTP_OK</body></html>",
        )
        .unwrap();

        // plain HTTP
        set_root(dir.clone());
        let mut srv = LiveServer::default();
        let port = srv
            .start(dir.clone(), false, 5540, &std::env::temp_dir())
            .expect("http start");
        let mut resp = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        resp.write_all(b"GET /index.html HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
            .unwrap();
        let mut buf = String::new();
        let _ = resp.read_to_string(&mut buf);
        srv.stop();
        assert!(
            buf.contains("200 OK") && buf.contains("REA_HTTP_OK"),
            "HTTP response: {buf}"
        );

        // HTTPS — unix: rustls backend (v1.8)
        #[cfg(unix)]
        {
            set_root(dir.clone());
            let mut srv2 = LiveServer::default();
            let port2 = srv2
                .start(dir, true, 5540, &std::env::temp_dir())
                .expect("https start");
            let out = std::process::Command::new("curl")
                .args([
                    "-sk",
                    "--max-time",
                    "8",
                    &format!("https://127.0.0.1:{port2}/index.html"),
                ])
                .output()
                .expect("curl");
            srv2.stop();
            let body = String::from_utf8_lossy(&out.stdout);
            assert!(body.contains("REA_HTTP_OK"), "HTTPS body: {body:?}");
        }
    }
}
