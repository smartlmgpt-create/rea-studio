//! Minimal DAP (Debug Adapter Protocol) client — integrated debugger.
//!
//! * Reads `launch.json` (workspace root or `.vscode/launch.json`) and maps
//!   `"type"` to a debug adapter executable:
//!     - "lldb"  -> lldb-dap        (Rust/C/C++ via LLVM)
//!     - "codelldb" -> adapter (not stdio; not supported — reported)
//!     - "python" -> python -m debugpy.adapter
//! * Speaks DAP over stdio (Content-Length framing, 100% Rust std).
//! * Supports: launch, breakpoints, continue/step/next/stop, output console,
//!   stack frames (jump to file/line), variables (scopes, 2 levels).

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

// ---------------------------------------------------------------- model

/// One breakpoint. `line` is 0-BASED (internal); converted to 1-based when
/// sent over DAP. `condition` is an optional expression (conditional bp).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Bp {
    pub line: usize,
    #[serde(default)]
    pub condition: Option<String>,
}

impl Bp {
    pub fn simple(line: usize) -> Self {
        Self { line, condition: None }
    }
}

#[derive(Clone, Debug)]
pub struct StackFrame {
    pub id: u64,
    pub name: String,
    pub file: Option<std::path::PathBuf>,
    pub line: usize,
}

#[derive(Clone, Debug)]
pub struct Variable {
    pub name: String,
    pub value: String,
    pub typ: String,
    pub variables_ref: u64,
}

#[derive(Clone, Copy, PartialEq)]
pub enum DbgState {
    Idle,
    Running,
    Paused,
    Terminated,
}

pub struct DapSession {
    pub state: DbgState,
    pub name: String,
    child: Child,
    stdin: Arc<Mutex<std::process::ChildStdin>>,
    inbox: Arc<Mutex<Vec<Value>>>,
    alive: Arc<AtomicBool>,
    next_id: Arc<AtomicU64>,
    /// pending request ids -> method (to route responses)
    pub frames: Vec<StackFrame>,
    pub variables: Vec<Variable>,
    pub console: String,
    pub current_frame: usize,
    /// in-flight evaluate requests: id -> (expression, context)
    pending_evals: HashMap<u64, (String, String)>,
    /// persistent watch results: expression -> latest value
    pub watches: Vec<(String, String)>,
    /// latest hover-eval result (expression, value)
    pub hover_result: Option<(String, String)>,
    /// "Break on exceptions" — filter ids taken from adapter capabilities
    pub exc_filters: Vec<String>,
    pub break_on_exceptions: bool,
}

// ---------------------------------------------------------------- launch.json

#[derive(Clone, Debug)]
pub struct LaunchCfg {
    pub name: String,
    pub adapter: String,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub env: HashMap<String, String>,
}

/// v1.7.3 — pending "trust this workspace?" request. launch.json may point
/// at ANY executable, so cloning a hostile repo and pressing F5 must never
/// run code silently: the first debug start per folder shows a consent
/// dialog listing exactly what would be executed.
#[derive(Clone, Debug)]
pub struct TrustReq {
    /// canonical, lower-cased workspace path (approval key)
    pub key: String,
    /// original path for display
    pub root_display: String,
    /// configurations the file asks to run
    pub cfgs: Vec<LaunchCfg>,
}

pub fn read_launch_json(ws: &std::path::Path) -> Result<Vec<LaunchCfg>, String> {
    let candidates = [ws.join(".vscode").join("launch.json"), ws.join("launch.json")];
    let raw = candidates
        .iter()
        .find(|p| p.exists())
        .ok_or("No launch.json found in the workspace (create one with a \"type\" of lldb or python)")?;
    let mut text = std::fs::read_to_string(raw).map_err(|e| e.to_string())?;
    // tolerate comments (VS Code launch.json is JSONC)
    strip_comments(&mut text);
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("launch.json: {e}"))?;
    let cfgs = v
        .get("configurations")
        .and_then(|c| c.as_array())
        .ok_or("launch.json has no \"configurations\"")?;
    let mut out = Vec::new();
    for c in cfgs {
        let adapter = c["type"].as_str().unwrap_or_default().to_string();
        if adapter.is_empty() {
            continue;
        }
        out.push(LaunchCfg {
            name: c["name"].as_str().unwrap_or("debug").to_string(),
            adapter,
            program: c["program"].as_str().unwrap_or_default().to_string(),
            args: c["args"]
                .as_array()
                .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                .unwrap_or_default(),
            cwd: c["cwd"].as_str().map(String::from),
            env: c["env"]
                .as_object()
                .map(|m| m.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string()))).collect())
                .unwrap_or_default(),
        });
    }
    if out.is_empty() {
        return Err("launch.json has no usable configuration".into());
    }
    Ok(out)
}

fn strip_comments(s: &mut String) {
    // crude but effective: remove /*…*/ and // comments outside strings
    let mut out = String::with_capacity(s.len());
    let mut in_str = false;
    let mut esc = false;
    let bytes: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if in_str {
            out.push(c);
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if c == '"' {
            in_str = true;
            out.push(c);
            i += 1;
            continue;
        }
        if c == '/' && i + 1 < bytes.len() && bytes[i + 1] == '/' {
            while i < bytes.len() && bytes[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && i + 1 < bytes.len() && bytes[i + 1] == '*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == '*' && bytes[i + 1] == '/') {
                i += 1;
            }
            i += 2;
            continue;
        }
        out.push(c);
        i += 1;
    }
    *s = out;
}

pub fn adapter_command(adapter: &str) -> Option<Vec<String>> {
    match adapter {
        "lldb" | "rust" | "cppdbg" | "c" | "cpp" | "rust-gdb" => Some(vec!["lldb-dap".into()]),
        "python" | "py" | "debugpy" => Some(vec![
            "python".into(),
            "-m".into(),
            "debugpy.adapter".into(),
        ]),
        "node" | "js" => Some(vec!["js-dap".into()]),
        _ => None,
    }
}

// ---------------------------------------------------------------- session

impl DapSession {
    pub fn spawn(cfg: &LaunchCfg) -> Result<Self, String> {
        let cmd = adapter_command(&cfg.adapter)
            .ok_or_else(|| format!("No stdio debug adapter mapped for type \"{}\" (supported: lldb, python)", cfg.adapter))?;
        let prog = &cmd[0];
        let mut c = Command::new(prog);
        c.args(&cmd[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000);
        }
        let mut child = c
            .spawn()
            .map_err(|e| format!("Cannot start adapter '{prog}': {e}"))?;
        let stdin = child.stdin.take().expect("dap stdin");
        let stdout = child.stdout.take().expect("dap stdout");

        let inbox: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
        let alive = Arc::new(AtomicBool::new(true));
        {
            let inbox = Arc::clone(&inbox);
            let alive = Arc::clone(&alive);
            std::thread::spawn(move || {
                let mut r = BufReader::new(stdout);
                loop {
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
                        inbox.lock().unwrap().push(v);
                    }
                }
                alive.store(false, Ordering::SeqCst);
            });
        }

        let mut s = Self {
            state: DbgState::Running,
            name: cfg.name.clone(),
            child,
            stdin: Arc::new(Mutex::new(stdin)),
            inbox,
            alive: Arc::clone(&alive),
            next_id: Arc::new(AtomicU64::new(1)),
            frames: Vec::new(),
            variables: Vec::new(),
            console: String::new(),
            current_frame: 0,
            pending_evals: HashMap::new(),
            watches: Vec::new(),
            hover_result: None,
            exc_filters: Vec::new(),
            break_on_exceptions: false,
        };
        s.send("initialize", json!({"adapterID": cfg.adapter, "request": "launch"}), true);
        let mut launch = json!({
            "request": "launch",
            "program": cfg.program,
            "args": cfg.args,
        });
        if let Some(cwd) = &cfg.cwd {
            launch["cwd"] = json!(cwd);
        }
        if !cfg.env.is_empty() {
            launch["env"] = json!(cfg.env);
        }
        s.send("launch", launch, true);
        Ok(s)
    }

    fn send(&mut self, method: &str, params: Value, wait_queue: bool) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let msg = json!({"seq": id, "type": "request", "command": method, "arguments": params});
        let _ = wait_queue;
        let _ = write_msg(&mut self.stdin.lock().unwrap(), &msg);
        id
    }

    pub fn req(&mut self, method: &str, params: Value) {
        self.send(method, params, false);
    }

    /// Evaluate an expression in the current frame (watch / hover-eval).
    /// The response arrives async and lands in `self.watches`.
    pub fn evaluate(&mut self, expr: &str, context: &str, frame_id: u64) {
        let mut args = json!({"expression": expr, "context": context});
        if frame_id > 0 {
            args["frameId"] = json!(frame_id);
        }
        let id = self.send("evaluate", args, false);
        self.pending_evals.insert(id, (expr.to_string(), context.to_string()));
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    pub fn drain(&mut self) -> Vec<Value> {
        self.inbox.lock().unwrap().drain(..).collect()
    }

    pub fn log(&mut self, line: &str) {
        self.console.push_str(line);
        if !self.console.ends_with('\n') {
            self.console.push('\n');
        }
        if self.console.len() > 120_000 {
            let cut = self.console.len() - 80_000;
            // floor `cut` to a char boundary — console text may be multi-byte
            let cut = (cut..=self.console.len())
                .find(|&i| self.console.is_char_boundary(i))
                .unwrap_or(self.console.len());
            let idx = self.console[cut..].find('\n').map(|i| cut + i + 1).unwrap_or(cut);
            self.console.drain(..idx);
        }
    }

    pub fn stop(&mut self) {
        let _ = write_msg(&mut self.stdin.lock().unwrap(), &json!({"seq":0,"type":"request","command":"disconnect","arguments":{"terminateDebuggee":true}}));
        let _ = self.child.kill();
        self.state = DbgState::Terminated;
    }
}

impl Drop for DapSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn write_msg(stdin: &mut std::process::ChildStdin, v: &Value) -> std::io::Result<()> {
    let body = serde_json::to_string(v).unwrap_or_default();
    write!(stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body)?;
    stdin.flush()
}

// ---------------------------------------------------------------- events

/// Process one inbound DAP message on the session + app-facing state.
/// Returns Option<(file, line)> when the debugger paused on a location.
pub fn handle_message(
    s: &mut DapSession,
    v: Value,
    breakpoints: &HashMap<std::path::PathBuf, Vec<crate::dap::Bp>>,
) -> Option<(std::path::PathBuf, usize)> {
    let typ = v["type"].as_str().unwrap_or("");
    match typ {
        "event" => {
            let ev = v["event"].as_str().unwrap_or("");
            match ev {
                "output" => {
                    let text = v["body"]["output"].as_str().unwrap_or("");
                    s.log(text);
                }
                "stopped" => {
                    s.state = DbgState::Paused;
                    s.req("threads", json!({}));
                }
                "continued" => {
                    s.state = DbgState::Running;
                    s.frames.clear();
                    s.variables.clear();
                }
                "terminated" | "exited" => {
                    s.state = DbgState::Terminated;
                    s.log("\n[program terminated]\n");
                }
                "breakpoint" => {}
                _ => {}
            }
            None
        }
        "response" => {
            let cmd = v["command"].as_str().unwrap_or("");
            let success = v["success"].as_bool().unwrap_or(false);
            match cmd {
                "initialize" => {
                    if success {
                        // adapter capabilities: exception breakpoint filters
                        let filters: Vec<String> = v["body"]["capabilities"]["exceptionBreakpointFilters"]
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .filter_map(|f| f["filter"].as_str().map(String::from))
                                    .collect()
                            })
                            .unwrap_or_default();
                        s.exc_filters = filters;
                        if s.break_on_exceptions && !s.exc_filters.is_empty() {
                            s.req(
                                "setExceptionBreakpoints",
                                json!({"filters": s.exc_filters}),
                            );
                        }
                        // push breakpoints for every file that has them
                        for (path, bps) in breakpoints {
                            if bps.is_empty() {
                                continue;
                            }
                            // DAP lines are 1-BASED — we store 0-based rows
                            let bps_json: Vec<Value> = bps
                                .iter()
                                .map(|bp| {
                                    let mut j = json!({"line": bp.line + 1});
                                    if let Some(cond) = &bp.condition {
                                        j["condition"] = json!(cond);
                                    }
                                    j
                                })
                                .collect();
                            s.req(
                                "setBreakpoints",
                                json!({
                                    "source": {"path": path.display().to_string()},
                                    "breakpoints": bps_json,
                                }),
                            );
                        }
                        s.req("configurationDone", json!({}));
                    } else {
                        s.log("[adapter initialize failed]\n");
                        s.state = DbgState::Terminated;
                    }
                    None
                }
                "threads" => {
                    if success {
                        if let Some(tid) = v["body"]["threads"]
                            .as_array()
                            .and_then(|a| a.first())
                            .and_then(|t| t["id"].as_i64())
                        {
                            s.req("stackTrace", json!({"threadId": tid, "levels": 20}));
                        }
                    }
                    None
                }
                "stackTrace" => {
                    let mut paused_loc = None;
                    if success {
                        s.frames.clear();
                        for f in v["body"]["stackFrames"].as_array().unwrap_or(&vec![]).iter() {
                            let file = f["source"]["path"]
                                .as_str()
                                .map(std::path::PathBuf::from);
                            let line = f["line"].as_u64().unwrap_or(0) as usize;
                            if paused_loc.is_none() {
                                if let Some(fp) = &file {
                                    paused_loc = Some((fp.clone(), line));
                                }
                            }
                            s.frames.push(StackFrame {
                                id: f["id"].as_u64().unwrap_or(0),
                                name: f["name"].as_str().unwrap_or("?").to_string(),
                                file,
                                line,
                            });
                        }
                        // fetch variables for the first frame
                        if let Some(fr) = s.frames.first() {
                            s.req("scopes", json!({"frameId": fr.id}));
                        }
                    }
                    paused_loc
                }
                "scopes" => {
                    if success {
                        if let Some(scope) = v["body"]["scopes"].as_array().and_then(|a| a.first()) {
                            if let Some(vref) = scope["variablesReference"].as_u64() {
                                s.req("variables", json!({"variablesReference": vref}));
                            }
                        }
                    }
                    None
                }
                "variables" => {
                    if success {
                        s.variables.clear();
                        for vr in v["body"]["variables"].as_array().unwrap_or(&vec![]).iter() {
                            s.variables.push(Variable {
                                name: vr["name"].as_str().unwrap_or("?").to_string(),
                                value: vr["value"].as_str().unwrap_or("").to_string(),
                                typ: vr["type"].as_str().unwrap_or("").to_string(),
                                variables_ref: vr["variablesReference"].as_u64().unwrap_or(0),
                            });
                        }
                    }
                    None
                }
                "evaluate" => {
                    let rid = v.get("id").and_then(|i| i.as_u64()).unwrap_or(0);
                    if let Some((expr, ctx)) = s.pending_evals.remove(&rid) {
                        let value = if success {
                            v["body"]["result"].as_str().unwrap_or("").to_string()
                        } else {
                            v["message"].as_str().unwrap_or("not available").to_string()
                        };
                        if ctx == "hover" {
                            s.hover_result = Some((expr, value));
                        } else {
                            // upsert the watch entry
                            if let Some(w) = s.watches.iter_mut().find(|(e, _)| *e == expr) {
                                w.1 = value;
                            } else {
                                s.watches.push((expr, value));
                            }
                        }
                    }
                    None
                }
                _ => {
                    if !success {
                        if let Some(msg) = v["message"].as_str() {
                            s.log(&format!("[error] {msg}\n"));
                        }
                    }
                    None
                }
            }
        }
        _ => None,
    }
}

// ---------------------------------------------------------------- UI

use eframe::egui::{self, Align, Rounding, Stroke, vec2};

use crate::app::App;
use crate::theme::Theme;

/// Floating debug toolbar at the top-center of the editor + bottom console.
pub fn show_ui(app: &mut App, ctx: &egui::Context, t: &Theme) {
    let Some(_sess) = app.debug.as_ref() else { return };

    // live-update exception breakpoints when the checkbox flips
    {
        let want = app.debug.as_ref().map(|s| s.break_on_exceptions).unwrap_or(false);
        if want != app.exc_sent {
            app.exc_sent = want;
            if let Some(s) = app.debug.as_mut() {
                let filters = if want { s.exc_filters.clone() } else { Vec::new() };
                s.req("setExceptionBreakpoints", json!({"filters": filters}));
            }
        }
    }

    // ---- toolbar ----
    let er = app.editor_rect;
    if er.width() > 100.0 {
        let btns = 6.0_f32;
        let w = btns * 34.0 + 16.0;
        let pos = pos_at(er.center().x - w * 0.5, er.top() + 8.0);
        egui::Area::new(egui::Id::new("rea_dbg_bar"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .movable(false)
            .show(ctx, |ui| {
                egui::Frame::default()
                    .fill(t.bg)
                    .stroke(Stroke::new(1.0, t.border))
                    .rounding(Rounding::same(8.0))
                    .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                    .shadow(egui::Shadow { offset: vec2(0.0, 4.0), blur: 12.0, spread: 0.0, color: t.shadow_col })
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let paused = app.debug.as_ref().map(|s| s.state == DbgState::Paused).unwrap_or(false);
                            if ui.add_enabled(paused, egui::Button::new(egui::RichText::new("▶ Continue").size(11.0))).clicked() {
                                app.debug_continue();
                            }
                            if ui.add_enabled(paused, egui::Button::new(egui::RichText::new("⤵ Step").size(11.0))).clicked() {
                                app.debug_step(true);
                            }
                            if ui.add_enabled(paused, egui::Button::new(egui::RichText::new("↓ Into").size(11.0))).clicked() {
                                app.debug_step(false);
                            }
                            if ui.add(egui::Button::new(egui::RichText::new("⟳ Restart").size(11.0))).clicked() {
                                app.debug_restart();
                            }
                            if ui.add(egui::Button::new(egui::RichText::new("■ Stop").size(11.0))).clicked() {
                                app.debug_stop();
                            }
                            let state = app.debug.as_ref().map(|s| s.state).unwrap_or(DbgState::Idle);
                            let (label, col) = match state {
                                DbgState::Running => ("running", t.ok),
                                DbgState::Paused => ("paused", t.warn),
                                DbgState::Terminated => ("ended", t.text_faint),
                                DbgState::Idle => ("", t.text_faint),
                            };
                            if !label.is_empty() {
                                ui.label(egui::RichText::new(label).size(10.5).color(col));
                            }
                        });
                    });
            });
    }

    // ---- bottom console panel ----
    let screen_h = ctx.screen_rect().height();
    egui::TopBottomPanel::bottom("rea_debug_console")
        .resizable(true)
        .default_height((screen_h * 0.22).clamp(110.0, 320.0))
        .min_height(80.0)
        .frame(egui::Frame::none().fill(t.panel))
        .show(ctx, |ui| {
            ui.vertical(|ui| {
                egui::Frame::default()
                    .fill(t.panel2)
                    .inner_margin(egui::Margin::symmetric(10.0, 4.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("DEBUG CONSOLE").size(10.5).strong().color(t.text_dim));
                            if let Some(s) = app.debug.as_ref() {
                                ui.label(egui::RichText::new(&s.name).size(10.5).color(t.text_faint));
                            }
                            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                                let mut exc = app.debug.as_ref().map(|s| s.break_on_exceptions).unwrap_or(false);
                                if ui.checkbox(&mut exc, "Break on exceptions").changed() {
                                    if let Some(s) = app.debug.as_mut() {
                                        s.break_on_exceptions = exc;
                                    }
                                }
                                if ui.add(egui::Button::new(egui::RichText::new("✕").size(11.0))).clicked() {
                                    app.debug_stop();
                                    app.debug = None;
                                }
                            });
                        });
                    });
                ui.separator();
                ui.columns(2, |cols| {
                    // left: console output
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .stick_to_bottom(true)
                        .show(&mut cols[0], |ui| {
                            let text = app.debug.as_ref().map(|s| s.console.clone()).unwrap_or_default();
                            ui.add(
                                egui::TextEdit::multiline(&mut text.clone())
                                    .font(egui::TextStyle::Monospace)
                                    .desired_rows(4)
                                    .desired_width(f32::INFINITY)
                                    .frame(false)
                                    .interactive(false),
                            );
                        });
                    // right: variables + stack + watch
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(&mut cols[1], |ui| {
                            // ---- watch input (evaluate in current frame) ----
                            let frame_id = app.debug.as_ref()
                                .and_then(|s| s.frames.get(s.current_frame))
                                .map(|f| f.id)
                                .unwrap_or(0);
                            let paused = app.debug.as_ref().map(|s| s.state == DbgState::Paused).unwrap_or(false);
                            let mut wexpr = app.watch_input.clone();
                            let wresp = ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("WATCH").size(10.0).strong().color(t.text_faint));
                                ui.add(
                                    egui::TextEdit::singleline(&mut wexpr)
                                        .hint_text(if paused { "expression, Enter to evaluate…" } else { "pause to evaluate…" })
                                        .desired_width(ui.available_width())
                                        .font(egui::TextStyle::Monospace),
                                )
                            });
                            if wresp.inner.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                                && paused
                                && !wexpr.trim().is_empty()
                            {
                                let expr = wexpr.trim().to_string();
                                app.debug_watch(&expr, frame_id);
                                app.watch_input.clear();
                            } else if wresp.inner.changed() {
                                app.watch_input = wexpr;
                            }
                            // ---- watch results ----
                            let watches = app.debug.as_ref().map(|s| s.watches.clone()).unwrap_or_default();
                            for (expr, val) in &watches {
                                ui.horizontal(|ui| {
                                    ui.monospace(egui::RichText::new(expr.as_str()).size(11.0).color(t.warn));
                                    ui.monospace(egui::RichText::new("=").size(11.0).color(t.text_faint));
                                    ui.monospace(egui::RichText::new(val.as_str()).size(11.0).color(t.text));
                                    if ui.small_button("✕").clicked() {
                                        app.debug_remove_watch(expr);
                                    }
                                });
                            }
                            let frames: Vec<StackFrame> = app.debug.as_ref().map(|s| s.frames.clone()).unwrap_or_default();
                            let cur_frame = app.debug.as_ref().map(|s| s.current_frame).unwrap_or(0);
                            if !frames.is_empty() {
                                {
                                    ui.label(egui::RichText::new("CALL STACK").size(10.0).strong().color(t.text_faint));
                                    for (i, f) in frames.iter().enumerate() {
                                        let sel = i == cur_frame;
                                        let lbl = format!("{}  ({}:{})", f.name, f.file.as_ref().map(|x| x.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()).unwrap_or_default(), f.line);
                                        if ui.add(egui::SelectableLabel::new(sel, egui::RichText::new(&lbl).size(11.0).color(if sel { t.accent } else { t.text_dim }))).clicked() {
                                            app.debug_select_frame(i);
                                        }
                                    }
                                    ui.add_space(6.0);
                                }
                                ui.label(egui::RichText::new("VARIABLES").size(10.0).strong().color(t.text_faint));
                                let vars: Vec<Variable> = app.debug.as_ref().map(|s| s.variables.clone()).unwrap_or_default();
                                if vars.is_empty() {
                                    ui.label(egui::RichText::new("— paused state shows locals here").size(10.5).color(t.text_faint));
                                }
                                for v in vars {
                                    ui.horizontal(|ui| {
                                        ui.monospace(egui::RichText::new(&v.name).size(11.0).color(t.accent));
                                        ui.monospace(egui::RichText::new(&v.value).size(11.0).color(t.text));
                                        if !v.typ.is_empty() {
                                            ui.monospace(egui::RichText::new(v.typ).size(10.0).color(t.text_faint));
                                        }
                                    });
                                }
                            }
                        });
                });
            });
        });

    // keep polling
    ctx.request_repaint_after(std::time::Duration::from_millis(120));
}

fn pos_at(x: f32, y: f32) -> egui::Pos2 {
    egui::pos2(x, y)
}

// ---------------------------------------------------------------- trust gate

/// Modal consent dialog shown before the first launch.json run in a folder.
/// Rendered every frame; a cheap no-op while `dap_trust_req` is None.
pub fn show_trust_modal(app: &mut App, ctx: &egui::Context, t: &Theme) {
    if app.dap_trust_req.is_none() {
        return;
    }
    egui::Area::new(egui::Id::new("rea_dap_trust"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .movable(false)
        .show(ctx, |ui| {
            egui::Frame::default()
                .fill(t.bg)
                .stroke(Stroke::new(1.0, t.border))
                .rounding(Rounding::same(10.0))
                .inner_margin(egui::Margin::same(14.0))
                .shadow(egui::Shadow {
                    offset: vec2(0.0, 6.0),
                    blur: 18.0,
                    spread: 0.0,
                    color: t.shadow_col,
                })
                .show(ui, |ui| {
                    ui.set_width(470.0);
                    ui.label(
                        egui::RichText::new("⚠ Trust this workspace?")
                            .size(15.0)
                            .strong(),
                    );
                    ui.add_space(6.0);
                    let Some(req) = app.dap_trust_req.clone() else { return };
                    ui.label(
                        egui::RichText::new(format!(
                            "launch.json in\n{}\nasks REA Studio to run:",
                            req.root_display
                        ))
                        .size(12.0),
                    );
                    ui.add_space(6.0);
                    egui::ScrollArea::vertical().max_height(150.0).show(ui, |ui| {
                        for c in &req.cfgs {
                            let mut args = c.program.clone();
                            for a in &c.args {
                                args.push(' ');
                                args.push_str(a);
                            }
                            ui.label(
                                egui::RichText::new(format!("● {}  ({})", c.name, c.adapter))
                                    .size(11.5)
                                    .color(t.accent)
                                    .strong(),
                            );
                            ui.label(egui::RichText::new(&args).monospace().size(11.0));
                            if let Some(cwd) = &c.cwd {
                                ui.label(
                                    egui::RichText::new(format!("cwd: {cwd}"))
                                        .monospace()
                                        .size(10.5)
                                        .color(t.text_dim),
                                );
                            }
                            ui.add_space(4.0);
                        }
                    });
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(
                            "Only run launch.json from folders you trust — a hostile \
                             repository can start any program on your machine.",
                        )
                        .size(11.0)
                        .color(t.text_dim),
                    );
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui
                            .add(egui::Button::new(
                                egui::RichText::new("Trust folder & Run").size(12.0),
                            ))
                            .clicked()
                        {
                            app.dap_trust_allow();
                        }
                        if ui
                            .add(egui::Button::new(
                                egui::RichText::new("Run just once").size(12.0),
                            ))
                            .clicked()
                        {
                            app.dap_trust_once();
                        }
                        if ui
                            .add(egui::Button::new(egui::RichText::new("Cancel").size(12.0)))
                            .clicked()
                        {
                            app.dap_trust_cancel();
                        }
                    });
                });
        });
}
