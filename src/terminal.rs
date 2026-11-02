//! Bottom panel — TERMINAL (v1.7: multi-session + real ConPTY) + REST + GIT.
//!
//! v1.7 redesign:
//!  * MULTIPLE terminal sessions with tabs (＋ / ×), PowerShell / cmd / bash
//!  * REAL ConPTY pseudo console: full-screen TUI programs (vim, less, htop)
//!    render through a VT emulator and interactive prompts (password, y/n)
//!    work — the old pipe-based backend stays as a fallback (Win < 1809)
//!  * Keyboard input goes straight to the shell (arrow history, Ctrl+C
//!    interrupt, Ctrl+V paste, Tab completion all handled by the shell)
//!  * Scrollback with the mouse wheel; ANSI colors incl. 256/truecolor
//!  * Bottom-panel tabs: TERMINAL / REST CLIENT / GIT (new git panel)

use std::io::{Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui;
use egui::{Color32, FontId, TextStyle, pos2, vec2};

use crate::app::{App, BottomTab};
use crate::theme::Theme;

#[cfg(windows)]
use crate::conpty::ConPtySession;
#[cfg(unix)]
use crate::pty::PtySession;

/// The real pseudo-console backend on this platform — ConPTY on Windows,
/// POSIX PTY on unix (v1.8). Identical API surface by design, so the whole
/// dispatch + renderer below is shared via this alias.
#[cfg(windows)]
type RealPty = ConPtySession;
#[cfg(unix)]
type RealPty = PtySession;

/// Which shell a terminal session runs. Defined here (not in conpty.rs) so
/// the session model compiles on every target — ConPTY itself is Windows-only.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShellKind {
    PowerShell,
    Cmd,
    GitBash,
}

impl ShellKind {
    pub fn label(&self) -> &'static str {
        match self {
            #[cfg(windows)]
            ShellKind::PowerShell => "PowerShell",
            #[cfg(windows)]
            ShellKind::Cmd => "cmd",
            #[cfg(windows)]
            ShellKind::GitBash => "bash",
            #[cfg(not(windows))]
            ShellKind::PowerShell => "shell",
            #[cfg(not(windows))]
            ShellKind::Cmd => "bash",
            #[cfg(not(windows))]
            ShellKind::GitBash => "bash -l",
        }
    }
}

pub(crate) const MARKER_CMD: &str =
    "Write-Output (\"__REA__\" + (Get-Location).Path + \"__END__\")";

/// Quick commands offered as one-click chips above the input row.
#[cfg(windows)]
const QUICK_CMDS: &[&str] = &[
    "dir",
    "git status",
    "git log --oneline -5",
    "python --version",
    "node -v",
    "cargo --version",
];
/// v1.8: unix-flavoured quick commands
#[cfg(not(windows))]
const QUICK_CMDS: &[&str] = &[
    "ls -la",
    "git status",
    "git log --oneline -5",
    "python3 --version",
    "node -v",
    "cargo --version",
];

// ---------------------------------------------------------------- legacy proc (fallback)

/// Remove ANSI escape sequences (CSI + OSC). Kept for the legacy backend.
fn strip_ansi(s: &str) -> String {
    if !s.contains('\u{1b}') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        if chars.peek() == Some(&'[') {
            chars.next();
            for n in chars.by_ref() {
                if n.is_ascii_alphabetic() {
                    break;
                }
            }
        } else if chars.peek() == Some(&']') {
            for n in chars.by_ref() {
                if n == '\u{7}' {
                    break;
                }
            }
        }
    }
    out
}

/// v1.5/v1.6 pipe-based PowerShell (fallback when ConPTY is unavailable).
pub struct TerminalProc {
    pub child: Child,
    stdin: ChildStdin,
    buf: Arc<Mutex<String>>,
    cwd: Arc<Mutex<String>>,
    /// partial (line-incomplete) output — displayed live while a long-running
    /// command is still producing it, instead of appearing only at the end
    partial: Arc<Mutex<String>>,
    #[allow(dead_code)]
    alive: Arc<AtomicBool>,
}

impl TerminalProc {
    pub fn spawn(cwd_dir: Option<&std::path::Path>) -> std::io::Result<Self> {
        let mut cmd = Command::new("powershell.exe");
        cmd.args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
        if let Some(d) = cwd_dir {
            if d.exists() {
                cmd.current_dir(d);
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = cmd.spawn()?;
        let stdin = child.stdin.take().expect("stdin");
        let stdout = child.stdout.take().expect("stdout");
        let stderr = child.stderr.take().expect("stderr");

        let buf: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let partial: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let cwd: Arc<Mutex<String>> = Arc::new(Mutex::new(
            cwd_dir.map(|p| p.display().to_string()).unwrap_or_default(),
        ));
        let alive = Arc::new(AtomicBool::new(true));

        {
            let buf = Arc::clone(&buf);
            let cwd = Arc::clone(&cwd);
            let alive = Arc::clone(&alive);
            let partial = Arc::clone(&partial);
            std::thread::spawn(move || {
                let mut out = stdout;
                let mut tmp = [0u8; 4096];
                let mut pending = String::new();
                loop {
                    match out.read(&mut tmp) {
                        Ok(0) => break,
                        Ok(n) => {
                            pending.push_str(&String::from_utf8_lossy(&tmp[..n]));
                            while let Some(pos) = pending.find('\n') {
                                let line: String = pending.drain(..pos + 1).collect();
                                let line = line.trim_end_matches(['\r', '\n']).to_string();
                                if let Some(p) = is_marker(&line) {
                                    *cwd.lock().unwrap() = p;
                                } else {
                                    let mut b = buf.lock().unwrap();
                                    b.push_str(&line);
                                    b.push('\n');
                                    cap_buffer(&mut b);
                                }
                            }
                            *partial.lock().unwrap() = pending.clone();
                        }
                        Err(_) => break,
                    }
                }
                let rest = strip_ansi(&pending).trim_end().to_string();
                if !rest.is_empty() && is_marker(&rest).is_none() {
                    let mut b = buf.lock().unwrap();
                    b.push_str(&pending);
                    b.push('\n');
                    cap_buffer(&mut b);
                }
                partial.lock().unwrap().clear();
                alive.store(false, Ordering::SeqCst);
            });
        }
        {
            let buf = Arc::clone(&buf);
            std::thread::spawn(move || {
                let mut err = stderr;
                let mut tmp = [0u8; 4096];
                loop {
                    match err.read(&mut tmp) {
                        Ok(0) => break,
                        Ok(n) => {
                            let mut b = buf.lock().unwrap();
                            b.push_str(&String::from_utf8_lossy(&tmp[..n]));
                            cap_buffer(&mut b);
                        }
                        Err(_) => break,
                    }
                }
            });
        }

        let mut term = Self { child, stdin, buf, cwd, partial, alive };

        let _ = term.send_raw("[Console]::OutputEncoding=[System.Text.Encoding]::UTF8; $ProgressPreference='SilentlyContinue'; $ErrorActionPreference='Continue'");
        let _ = term.send_raw("Write-Output ('REA Studio PowerShell ' + $PSVersionTable.PSVersion.ToString() + ' ready.')");
        let _ = term.send_raw(MARKER_CMD);
        Ok(term)
    }

    pub fn send_raw(&mut self, line: &str) -> std::io::Result<()> {
        self.stdin.write_all(line.as_bytes())?;
        self.stdin.write_all(b"\r\n")?;
        self.stdin.flush()
    }

    pub fn send_command(&mut self, cmd: &str) -> std::io::Result<()> {
        self.echo_cmd(cmd);
        self.send_raw(cmd)?;
        self.send_raw(MARKER_CMD)
    }

    pub fn echo_cmd(&self, cmd: &str) {
        let cwd = self.cwd.lock().unwrap().clone();
        let mut b = self.buf.lock().unwrap();
        b.push_str(&format!("PS {}> {}\n", short_path(&cwd), cmd));
        cap_buffer(&mut b);
    }

    pub fn text(&self) -> String {
        let b = self.buf.lock().unwrap().clone();
        let p = self.partial.lock().unwrap().clone();
        if p.is_empty() {
            b
        } else {
            format!("{b}{p}")
        }
    }

    pub fn cwd(&self) -> String {
        self.cwd.lock().unwrap().clone()
    }

    pub fn clear(&self) {
        self.buf.lock().unwrap().clear();
    }

    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

fn cap_buffer(b: &mut String) {
    if b.len() > 200_000 {
        let cut = b.len() - 150_000;
        let idx = b[cut..].find('\n').map(|i| cut + i + 1).unwrap_or(cut);
        b.drain(..idx);
    }
}

fn short_path(p: &str) -> String {
    let chars: Vec<char> = p.chars().collect();
    if chars.len() <= 30 {
        p.to_string()
    } else {
        format!("…{}", p[chars.len() - 29..].to_string())
    }
}

fn is_marker(line: &str) -> Option<String> {
    let l = line.trim();
    if l.starts_with("__REA__") && l.ends_with("__END__") {
        Some(l[7..l.len() - 7].to_string())
    } else {
        None
    }
}

impl Drop for TerminalProc {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

// ---------------------------------------------------------------- ANSI runs (legacy renderer)

/// One styled text run produced by the ANSI SGR parser.
struct AnsiRun {
    text: String,
    fg: Option<Color32>,
    bold: bool,
    italic: bool,
}

/// Parse SGR sequences into styled runs. Unknown escapes are dropped.
fn ansi_runs(s: &str, default_fg: Color32) -> Vec<AnsiRun> {
    let mut runs: Vec<AnsiRun> = Vec::new();
    let mut cur = AnsiRun { text: String::new(), fg: None, bold: false, italic: false };
    let mut chars = s.chars().peekable();
    let mut push = |cur: &mut AnsiRun, runs: &mut Vec<AnsiRun>| {
        if !cur.text.is_empty() {
            runs.push(AnsiRun {
                text: std::mem::take(&mut cur.text),
                fg: cur.fg,
                bold: cur.bold,
                italic: cur.italic,
            });
        }
    };
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            cur.text.push(c);
            continue;
        }
        if chars.peek() != Some(&'[') {
            continue; // not CSI — drop
        }
        chars.next();
        let mut params = String::new();
        let mut final_b = ' ';
        while let Some(&n) = chars.peek() {
            chars.next();
            if n.is_ascii_alphabetic() {
                final_b = n;
                break;
            }
            params.push(n);
        }
        if final_b != 'm' {
            continue; // only colors (SGR) are rendered
        }
        let codes: Vec<u32> = params
            .split(';')
            .filter_map(|p| p.parse::<u32>().ok())
            .collect();
        if codes.is_empty() {
            push(&mut cur, &mut runs);
            cur.fg = None;
            cur.bold = false;
            cur.italic = false;
            continue;
        }
        let mut i = 0;
        while i < codes.len() {
            match codes[i] {
                0 => {
                    push(&mut cur, &mut runs);
                    cur.fg = None;
                    cur.bold = false;
                    cur.italic = false;
                }
                1 | 3 | 22 | 23 | 30..=37 | 39 | 90..=97 => {
                    push(&mut cur, &mut runs);
                    match codes[i] {
                        1 => cur.bold = true,
                        3 => cur.italic = true,
                        22 => cur.bold = false,
                        23 => cur.italic = false,
                        30..=37 => cur.fg = Some(crate::vt::ansi_color((codes[i] - 30) as u8)),
                        39 => cur.fg = None,
                        _ => cur.fg = Some(crate::vt::ansi_color((codes[i] - 90 + 8) as u8)),
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }
    push(&mut cur, &mut runs);
    if runs.is_empty() {
        runs.push(AnsiRun { text: String::new(), fg: None, bold: false, italic: false });
    }
    let _ = default_fg;
    runs
}

// ---------------------------------------------------------------- sessions

/// One terminal tab. Backend: real ConPTY (preferred) or the legacy pipe.
pub struct TermSession {
    pub id: usize,
    pub kind: ShellKind,
    pub backend: Backend,
    pub scroll_off: usize,
    pub focused: bool,
    /// legacy-only: input line + history
    pub input: String,
    pub hist: Vec<String>,
    pub hist_idx: Option<usize>,
}

pub enum Backend {
    Real(Box<RealPty>),
    Legacy(Box<TerminalProc>),
}

pub fn shell_available(kind: ShellKind) -> bool {
    match kind {
        ShellKind::PowerShell => true,
        ShellKind::Cmd => true,
        ShellKind::GitBash => {
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                Command::new("where")
                    .args(["bash.exe"])
                    .creation_flags(0x0800_0000)
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false)
            }
            #[cfg(not(windows))]
            {
                // login bash — present on effectively every unix system
                true
            }
        }
    }
}

impl TermSession {
    pub fn label(&self) -> String {
        match &self.backend {
            Backend::Real(_) => format!("{} {}", self.kind.label(), self.id),
            Backend::Legacy(_) => format!("PS {}", self.id),
        }
    }

    pub fn is_alive(&mut self) -> bool {
        match &mut self.backend {
            Backend::Real(s) => !s.exited.load(Ordering::SeqCst),
            Backend::Legacy(s) => s.is_alive(),
        }
    }

    pub fn kill(&mut self) {
        match &mut self.backend {
            Backend::Real(s) => s.kill(),
            Backend::Legacy(s) => {
                let _ = s.child.kill();
            }
        }
    }
}

// ---------------------------------------------------------------- key mapping

/// Translate an egui key + modifiers into the VT byte sequence the conpty
/// child expects. Returns None when the key has no terminal meaning.
fn key_to_vt(key: egui::Key, mods: egui::Modifiers) -> Option<String> {
    use egui::Key;
    let c = mods.ctrl;
    let s = mods.shift;
    let a = mods.alt;
    let seq: Option<String> = match key {
        Key::Enter => Some("\r".into()),
        Key::Tab => {
            if s {
                Some("\u{1b}[Z".into()) // back-tab
            } else {
                Some("\t".into())
            }
        }
        Key::Backspace => Some("\u{7f}".into()),
        Key::Escape => Some("\u{1b}".into()),
        Key::ArrowUp => Some(if c { "\u{1b}[1;5A".into() } else if s { "\u{1b}[1;2A".into() } else { "\u{1b}[A".into() }),
        Key::ArrowDown => Some(if c { "\u{1b}[1;5B".into() } else { "\u{1b}[B".into() }),
        Key::ArrowRight => Some(if c { "\u{1b}[1;5C".into() } else { "\u{1b}[C".into() }),
        Key::ArrowLeft => Some(if c { "\u{1b}[1;5D".into() } else { "\u{1b}[D".into() }),
        Key::Home => Some("\u{1b}[H".into()),
        Key::End => Some("\u{1b}[F".into()),
        Key::Delete => Some("\u{1b}[3~".into()),
        Key::PageUp => Some("\u{1b}[5~".into()),
        Key::PageDown => Some("\u{1b}[6~".into()),
        Key::F1 => Some("\u{1b}OP".into()),
        Key::F2 => Some("\u{1b}OQ".into()),
        Key::F3 => Some("\u{1b}OR".into()),
        Key::F4 => Some("\u{1b}OS".into()),
        Key::F5 => Some("\u{1b}[15~".into()),
        Key::F6 => Some("\u{1b}[17~".into()),
        Key::F7 => Some("\u{1b}[18~".into()),
        Key::F8 => Some("\u{1b}[19~".into()),
        Key::F9 => Some("\u{1b}[20~".into()),
        Key::F10 => Some("\u{1b}[21~".into()),
        Key::F11 => Some("\u{1b}[23~".into()),
        Key::F12 => Some("\u{1b}[24~".into()),
        Key::Insert => Some("\u{1b}[2~".into()),
        _ => {
            // Ctrl+letter → control code
            if c && !a {
                match key {
                    Key::A => Some("\u{1}".to_string()),
                    Key::B => Some("\u{2}".to_string()),
                    Key::C => Some("\u{3}".to_string()),
                    Key::D => Some("\u{4}".to_string()),
                    Key::E => Some("\u{5}".to_string()),
                    Key::F => Some("\u{6}".to_string()),
                    Key::G => Some("\u{7}".to_string()),
                    Key::H => Some("\u{8}".to_string()),
                    // FIX: control codes are letter position in the alphabet
                    // (A=1 … Z=26). 13 of them were wrong — e.g. Ctrl+Z sent
                    // the digit '2', Ctrl+W sent a quote, Ctrl+U sent '!'.
                    Key::K => Some("\u{b}".to_string()),
                    Key::L => Some("\u{c}".to_string()),
                    Key::N => Some("\u{e}".to_string()),
                    Key::O => Some("\u{f}".to_string()),
                    Key::P => Some("\u{10}".to_string()),
                    Key::Q => Some("\u{11}".to_string()),
                    Key::R => Some("\u{12}".to_string()),
                    Key::S => Some("\u{13}".to_string()),
                    Key::T => Some("\u{14}".to_string()),
                    Key::U => Some("\u{15}".to_string()),
                    Key::V => None, // paste handled via Event::Paste
                    Key::W => Some("\u{17}".to_string()),
                    Key::X => Some("\u{18}".to_string()),
                    Key::Y => Some("\u{19}".to_string()),
                    Key::Z => Some("\u{1a}".to_string()),
                    _ => None,
                }
            } else {
                None
            }
        }
    };
    if a {
        // Alt+key → ESC prefix
        return seq.map(|q| format!("\u{1b}{q}"));
    }
    seq
}

// ---------------------------------------------------------------- panel

pub fn show(app: &mut App, ctx: &egui::Context, t: &Theme) {
    if !app.terminal_visible {
        return;
    }

    let screen_h = ctx.screen_rect().height();
    let mut restart = false;
    egui::TopBottomPanel::bottom("rea_terminal")
        .resizable(true)
        .default_height((screen_h * 0.30).clamp(150.0, 430.0))
        .height_range(90.0..=(screen_h * 0.75))
        .frame(
            egui::Frame::none()
                .fill(t.panel)
                .inner_margin(egui::Margin::same(0.0)),
        )
        .show(ctx, |ui| {
            ui.vertical(|ui| {
                // ---- header: tabs + live pill + actions ----
                egui::Frame::default()
                    .fill(t.panel2)
                    .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let tab = |ui: &mut egui::Ui, label: &str, active: bool| {
                                let txt = egui::RichText::new(label)
                                    .size(10.5)
                                    .strong()
                                    .color(if active { t.text } else { t.text_faint });
                                let btn = if active {
                                    egui::Button::new(txt).fill(t.panel).stroke(egui::Stroke::new(1.0, t.border))
                                } else {
                                    egui::Button::new(txt).fill(Color32::TRANSPARENT)
                                };
                                ui.add(btn)
                            };
                            if tab(ui, "⌘ TERMINAL", app.bottom_tab == BottomTab::Terminal).clicked() {
                                app.bottom_tab = BottomTab::Terminal;
                            }
                            if tab(ui, "⇅ REST CLIENT", app.bottom_tab == BottomTab::Rest).clicked() {
                                app.bottom_tab = BottomTab::Rest;
                            }
                            if tab(ui, "⑂ GIT", app.bottom_tab == BottomTab::Git).clicked() {
                                app.bottom_tab = BottomTab::Git;
                                app.git_refresh();
                            }

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                // close
                                let (xp, xr) = ui.allocate_exact_size(vec2(18.0, 18.0), egui::Sense::click());
                                if xr.hovered() {
                                    ui.painter().rect_filled(xp, 4.0, t.hover);
                                }
                                crate::icons::close_x(&ui.painter(), xp.center(), t.text_dim);
                                if xr.clicked() {
                                    app.terminal_visible = false;
                                    app.save_settings();
                                }
                                // clear (trash)
                                let (tp_, tr) = ui.allocate_exact_size(vec2(18.0, 18.0), egui::Sense::click());
                                if tr.hovered() {
                                    ui.painter().rect_filled(tp_, 4.0, t.hover);
                                }
                                let r = tp_;
                                let p = ui.painter();
                                let c = r.center();
                                p.rect_stroke(
                                    egui::Rect::from_min_size(c + vec2(-4.5, -3.0), vec2(9.0, 8.0)),
                                    1.5,
                                    egui::Stroke::new(1.2, t.text_dim),
                                );
                                p.line_segment([c + vec2(-2.0, -5.5), c + vec2(2.0, -5.5)], egui::Stroke::new(1.2, t.text_dim));
                                p.line_segment([c + vec2(-2.0, -5.5), c + vec2(-2.0, -3.0)], egui::Stroke::new(1.2, t.text_dim));
                                p.line_segment([c + vec2(2.0, -5.5), c + vec2(2.0, -3.0)], egui::Stroke::new(1.2, t.text_dim));
                                if tr.clicked() {
                                    match app.bottom_tab {
                                        BottomTab::Terminal => app.clear_terminal(),
                                        BottomTab::Rest => app.rest.results.clear(),
                                        BottomTab::Git => app.git_state.log.clear(),
                                    }
                                }
                                // stop/restart
                                let (sp_, sr) = ui.allocate_exact_size(vec2(18.0, 18.0), egui::Sense::click());
                                if sr.hovered() {
                                    ui.painter().rect_filled(sp_, 4.0, t.hover);
                                }
                                ui.painter().circle_filled(sr.rect.center() + vec2(-1.5, 0.0), 4.0, t.err);
                                ui.painter().circle_filled(sr.rect.center() + vec2(2.5, 0.0), 4.0, t.err.gamma_multiply(0.6));
                                if sr.clicked() {
                                    restart = true;
                                }
                                sr.on_hover_text("Stop (kill the running command and restart the terminal)");
                                // Live Server pill
                                ui.separator();
                                crate::liveserver::pill_ui(app, ui, t, t.text);
                            });
                        });
                    });

                // ---- content per tab ----
                match app.bottom_tab {
                    BottomTab::Rest => {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .max_height(ui.available_height())
                            .show(ui, |ui| {
                                egui::Frame::default()
                                    .fill(t.editor_bg)
                                    .inner_margin(egui::Margin::same(8.0))
                                    .show(ui, |ui| {
                                        crate::restclient::show(app, ui, t);
                                    });
                            });
                    }
                    BottomTab::Git => {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .max_height(ui.available_height())
                            .show(ui, |ui| {
                                egui::Frame::default()
                                    .fill(t.editor_bg)
                                    .inner_margin(egui::Margin::same(8.0))
                                    .show(ui, |ui| {
                                        crate::gitpanel::show(app, ui, t);
                                    });
                            });
                    }
                    BottomTab::Terminal => {
                        terminal_tab(app, ui, t, &mut restart);
                    }
                }
            });
        });

    if restart {
        app.restart_terminal();
    }

    // keep repainting so new output shows up
    let conpty_alive = app
        .terms
        .iter_mut()
        .any(|s| matches!(s.backend, Backend::Real(_)) && s.is_alive_silent());
    if conpty_alive || !app.terms.is_empty() || app.rest.busy || app.git_state.job.is_some() {
        ctx.request_repaint_after(std::time::Duration::from_millis(90));
    }
}

// ---------------------------------------------------------------- terminal tab

fn terminal_tab(app: &mut App, ui: &mut egui::Ui, t: &Theme, restart: &mut bool) {
    // FIX (focus): consume pending "focus the active session" requests (tab
    // click / panel toggle / restart). The old code SET this flag in five
    // places but nothing ever READ it, so switching tabs never actually
    // focused the session. Must run while all sessions are still in the vec.
    if app.term_focus_req {
        app.term_focus_req = false;
        for (i, s) in app.terms.iter_mut().enumerate() {
            s.focused = i == app.term_active;
        }
    }
    if app.terms.is_empty() {
        ui.add_space(10.0);
        ui.vertical_centered(|ui| {
            if ui.button("＋ New Terminal").clicked() {
                app.ensure_terminal();
            }
        });
        return;
    }

    // ---- session tabs row ----
    egui::Frame::default()
        .fill(t.panel)
        .inner_margin(egui::Margin::symmetric(6.0, 3.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let n = app.terms.len();
                let mut alive_flags = vec![true; n];
                for (i, s) in app.terms.iter_mut().enumerate() {
                    alive_flags[i] = s.is_alive_silent();
                }
                for i in 0..n {
                    let (label, alive, focused) = {
                        let s = &app.terms[i];
                        (s.label(), alive_flags[i], s.focused)
                    };
                    let active = i == app.term_active;
                    let txt = egui::RichText::new(format!(
                        "{}{}",
                        if alive { "" } else { "× " },
                        label
                    ))
                    .size(10.5)
                    .color(if active {
                        t.text
                    } else if alive {
                        t.text_dim
                    } else {
                        t.err
                    });
                    let mut btn = egui::Button::new(txt);
                    if active {
                        btn = btn.fill(t.panel2).stroke(egui::Stroke::new(1.0, if focused { t.accent } else { t.border }));
                    } else {
                        btn = btn.fill(Color32::TRANSPARENT);
                    }
                    if ui.add(btn).clicked() {
                        app.term_active = i;
                        app.term_focus_req = true;
                    }
                }
                // new session menu
                ui.separator();
                ui.menu_button(egui::RichText::new("＋").size(12.0), |ui| {
                    for kind in [ShellKind::PowerShell, ShellKind::Cmd, ShellKind::GitBash] {
                        let avail = shell_available(kind);
                        if ui
                            .add_enabled(avail, egui::Button::new(egui::RichText::new(kind.label()).size(11.0)))
                            .clicked()
                        {
                            app.spawn_terminal(kind);
                            ui.close_menu();
                        }
                    }
                    ui.separator();
                    #[cfg(windows)]
                    if ui.button(egui::RichText::new("Use legacy pipe mode (no ConPTY)").size(10.5)).clicked() {
                        app.spawn_terminal_legacy();
                        ui.close_menu();
                    }
                });
                // close active session
                if ui
                    .button(egui::RichText::new("✕").size(10.5).color(t.text_dim))
                    .on_hover_text("Close this terminal")
                    .clicked()
                {
                    app.close_terminal(app.term_active);
                    return;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let cwd = app.active_session_cwd().unwrap_or_default();
                    if !cwd.is_empty() {
                        ui.label(
                            egui::RichText::new(short_path(&cwd))
                                .size(9.5)
                                .color(t.text_faint),
                        );
                    }
                });
            });
        });
    ui.separator();

    // dead session bar
    let dead = app
        .terms
        .get_mut(app.term_active)
        .map(|s| !s.is_alive_silent())
        .unwrap_or(false);
    if dead {
        egui::Frame::default()
            .fill(t.err.gamma_multiply(0.2))
            .inner_margin(egui::Margin::symmetric(10.0, 5.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("This terminal exited.")
                            .size(12.0)
                            .color(t.err),
                    );
                    if ui.add(egui::Button::new(egui::RichText::new("Restart").size(11.5))).clicked() {
                        *restart = true;
                    }
                });
            });
    }

    if app.terms.is_empty() {
        // closing the last session left the vec empty — the old code fell
        // through to `app.terms.remove(0)` and panicked every frame
        return;
    }
    let _si = app.term_active.min(app.terms.len() - 1);

    // backend render (split borrow: take the session out of the vec)
    // FIX (focus): the session's own `focused` flag must be passed in —
    // `app.terms_focus_flag()` reads the vec WITHOUT the active session in
    // it (it was just removed for the split borrow), so it returned false
    // for the last tab and true only for the wrong session otherwise.
    // Result was: no cursor + keyboard input silently dropped = a terminal
    // you cannot type into.
    let mut session = app.terms.remove(_si);
    let TermSession { backend, input, hist, hist_idx, scroll_off, focused, .. } = &mut session;
    match backend {
        Backend::Real(s) => conpty_view(app, s, scroll_off, *focused, ui, t),
        Backend::Legacy(tp) => legacy_view(app, tp, input, hist, hist_idx, ui, t),
    }
    app.terms.insert(_si.min(app.terms.len()), session);

    // apply pending focus / scroll actions to the active session
    if let Some(f) = app.term_pending_focus.take() {
        if let Some(s) = app.terms.get_mut(app.term_active) {
            s.focused = f;
        }
    }
    if let Some(d) = app.term_pending_scroll.take() {
        if let Some(s) = app.terms.get_mut(app.term_active) {
            let sb = match &s.backend {
                Backend::Real(c) => c.vt.scrollback.len(),
                Backend::Legacy(_) => 0,
            };
            if d > 0 {
                s.scroll_off = (s.scroll_off + d as usize).min(sb);
            } else {
                s.scroll_off = s.scroll_off.saturating_sub((-d) as usize);
            }
        }
    }

    // ---- quick command chips ----
    egui::Frame::default()
        .fill(t.panel)
        .inner_margin(egui::Margin::symmetric(10.0, 3.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("quick:").size(9.5).color(t.text_faint));
                for qc in QUICK_CMDS {
                    if ui.button(egui::RichText::new(*qc).size(9.5).color(t.text_dim)).clicked() {
                        app.send_terminal(qc);
                    }
                }
            });
        });
}

// ---------------------------------------------------------------- conpty renderer

fn conpty_view(
    app: &mut App,
    s: &mut RealPty,
    scroll_off: &mut usize,
    focused: bool,
    ui: &mut egui::Ui,
    t: &Theme,
) {
    // pump new output into the vt + flush DSR/DA replies
    s.pump();

    let mono_size = 12.5;
    let font = FontId::monospace(mono_size);
    let (char_w, row_h) = ui.fonts(|f| {
        let g = f.layout_no_wrap("M".to_string(), font.clone(), t.text);
        (g.size().x.max(6.0), f.row_height(&font).max(15.0))
    });

    egui::Frame::default()
        .fill(t.editor_bg)
        .inner_margin(egui::Margin::same(6.0))
        .show(ui, |ui| {
            let avail = ui.available_size();
            let cols = ((avail.x - 4.0) / char_w).floor().max(20.0) as usize;
            let rows = ((avail.y - 4.0) / row_h).floor().max(6.0) as usize;
            s.resize(cols, rows);

            let (rect, resp) = ui.allocate_exact_size(avail, egui::Sense::click_and_drag());
            let hovered = resp.hovered();
            let clicked = resp.clicked();
            if clicked {
                s_focus(app, true);
            }
            // click elsewhere → unfocus
            let any_click_elsewhere = ui.input(|i| {
                i.pointer.any_click() && !rect.contains(i.pointer.interact_pos().unwrap_or_default())
            });
            if any_click_elsewhere {
                s_focus(app, false);
            }
            // `focused` now comes from the session itself (see call site) —
            // the old `app.terms_focus_flag()` read here was always wrong
            // because the session is not in `app.terms` during rendering.

            // wheel scrolling over scrollback (positive delta = scroll back)
            let wheel = ui.input(|i| i.raw_scroll_delta.y);
            if hovered && wheel != 0.0 {
                if wheel > 0.0 {
                    *scroll_off = (*scroll_off).saturating_add((wheel / row_h).ceil() as usize);
                } else {
                    *scroll_off = (*scroll_off).saturating_sub(((wheel / row_h).ceil().abs() as usize).min(*scroll_off));
                }
            }

            // paint grid (or scrollback view)
            let p = ui.painter_at(rect);
            let orig = rect.min;
            let sb = s.vt.scrollback.len();
            let alt = s.vt.alt_active;
            let off = if alt { 0 } else { (*scroll_off).min(sb) };
            let bg_default = t.editor_bg;

            let total = sb + s.vt.rows;
            let start = if alt || off == 0 {
                sb
            } else {
                total.saturating_sub(s.vt.rows + off)
            };

            for vr in 0..s.vt.rows {
                let idx = start + vr;
                let row: Vec<crate::vt::Cell> = if idx < sb {
                    s.vt.scrollback[idx].clone()
                } else if idx < total {
                    s.vt.row(idx - sb).to_vec()
                } else {
                    continue;
                };
                let y = orig.y + vr as f32 * row_h;
                // merge equal-style runs
                let mut cx = 0usize;
                while cx < row.len() {
                    let c0 = row[cx].clone();
                    let mut end = cx + 1;
                    while end < row.len() && row[end] == c0 {
                        end += 1;
                    }
                    let text: String = row[cx..end].iter().map(|c| c.ch).collect();
                    if !c0.is_blank() || c0.bg != Color32::TRANSPARENT {
                        if c0.bg != Color32::TRANSPARENT && c0.bg != bg_default {
                            p.rect_filled(
                                egui::Rect::from_min_size(
                                    pos2(orig.x + cx as f32 * char_w, y),
                                    vec2((end - cx) as f32 * char_w, row_h),
                                ),
                                0.0,
                                c0.bg,
                            );
                        }
                        if !c0.is_blank() {
                            let color = if c0.fg == Color32::TRANSPARENT { t.text } else { c0.fg };
                            let txt = text.trim_end().to_string();
                            if txt.is_empty() {
                                cx = end;
                                continue;
                            }
                            p.text(
                                pos2(orig.x + cx as f32 * char_w, y + row_h * 0.5),
                                egui::Align2::LEFT_CENTER,
                                txt.clone(),
                                font.clone(),
                                color,
                            );
                            if c0.bold {
                                // fake bold: redraw offset by half a pixel
                                p.text(
                                    pos2(orig.x + cx as f32 * char_w + 0.5, y + row_h * 0.5),
                                    egui::Align2::LEFT_CENTER,
                                    txt.clone(),
                                    font.clone(),
                                    color,
                                );
                            }
                            if c0.underline {
                                let w = text.trim_end().chars().count() as f32 * char_w;
                                p.line_segment(
                                    [
                                        pos2(orig.x + cx as f32 * char_w, y + row_h - 2.0),
                                        pos2(orig.x + cx as f32 * char_w + w, y + row_h - 2.0),
                                    ],
                                    egui::Stroke::new(1.0, color),
                                );
                            }
                        }
                    }
                    cx = end;
                }
            }

            // cursor
            if focused && s.vt.cursor_visible() && (alt || off == 0) {
                let (cr, cc) = s.vt.cursor();
                let crect = egui::Rect::from_min_size(
                    pos2(orig.x + cc as f32 * char_w, orig.y + cr as f32 * row_h),
                    vec2(char_w, row_h),
                );
                p.rect_stroke(crect, 2.0, egui::Stroke::new(1.4, t.accent));
            }
        });

    // ---- keyboard input when focused ----
    if focused {
        let (mut to_send, mut paste): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
        ui.input(|i| {
            for ev in &i.events {
                match ev {
                    egui::Event::Key { key, pressed: true, modifiers, .. } => {
                        if let Some(seq) = key_to_vt(*key, *modifiers) {
                            to_send.push(seq);
                        }
                    }
                    egui::Event::Text(txt) => {
                        to_send.push(txt.clone());
                    }
                    egui::Event::Paste(txt) => {
                        paste.push(txt.replace("\r\n", "\r").replace('\n', "\r"));
                    }
                    _ => {}
                }
            }
        });
        if !paste.is_empty() {
            for pt in paste {
                s.write_input(&pt);
            }
        }
        if !to_send.is_empty() {
            // typing resets scroll to live
            *scroll_off = 0;
            for seq in to_send {
                s.write_input(&seq);
            }
        }
    }

    // exited?
    if s.exited.load(Ordering::SeqCst) {
        // keep last frame visible; dead bar offers Restart
    }
}

// focus / scroll helpers operate on the ACTIVE session (already removed from
// the vec, so we track pending actions on App and apply after re-insert)

fn s_focus(app: &mut App, on: bool) {
    app.term_pending_focus = Some(on);
}
fn s_scroll(app: &mut App, delta: i32) {
    app.term_pending_scroll = Some(delta);
}

// ---------------------------------------------------------------- legacy renderer

fn legacy_view(
    app: &mut App,
    s: &mut TerminalProc,
    input: &mut String,
    hist: &mut Vec<String>,
    hist_idx: &mut Option<usize>,
    ui: &mut egui::Ui,
    t: &Theme,
) {
    let out_text = s.text();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            egui::Frame::default()
                .fill(t.editor_bg)
                .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    ui.horizontal_wrapped(|ui| {
                        for run in ansi_runs(&out_text, t.text) {
                            let col = run.fg.unwrap_or(t.text);
                            let mut rt = egui::RichText::new(&run.text)
                                .font(FontId::monospace(12.5))
                                .color(col);
                            if run.bold {
                                rt = rt.strong();
                            }
                            if run.italic {
                                rt = rt.italics();
                            }
                            ui.label(rt);
                        }
                    });
                });
        });

    // ---- input row with Up/Down history ----
    ui.separator();
    egui::Frame::default()
        .fill(t.panel)
        .inner_margin(egui::Margin::symmetric(10.0, 5.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let cwd = s.cwd();
                ui.label(
                    egui::RichText::new(format!("PS {}>", short_path(&cwd)))
                        .size(12.0)
                        .strong()
                        .color(t.ok),
                );
                let mut line = input.clone();
                let hist_up = ui.input(|i| i.key_pressed(egui::Key::ArrowUp));
                let hist_dn = ui.input(|i| i.key_pressed(egui::Key::ArrowDown));
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut line)
                        .hint_text("PowerShell command…  (↑/↓ history)")
                        .desired_width(ui.available_width())
                        .font(TextStyle::Monospace)
                        .frame(false),
                );
                let focused = resp.has_focus();
                let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if focused && hist_up && !hist.is_empty() {
                    let idx = match hist_idx {
                        None => hist.len() - 1,
                        Some(i) => i.saturating_sub(1),
                    };
                    *hist_idx = Some(idx);
                    *input = hist[idx].clone();
                    resp.request_focus();
                } else if focused && hist_dn {
                    if let Some(i) = *hist_idx {
                        if i + 1 < hist.len() {
                            *hist_idx = Some(i + 1);
                            *input = hist[i + 1].clone();
                        } else {
                            *hist_idx = None;
                            input.clear();
                        }
                        resp.request_focus();
                    }
                }
                if resp.changed() && !hist_up && !hist_dn {
                    *input = line;
                }
                if enter {
                    let cmd = input.trim().to_string();
                    input.clear();
                    if !cmd.is_empty() {
                        hist.push(cmd.clone());
                        if hist.len() > 100 {
                            hist.remove(0);
                        }
                    }
                    *hist_idx = None;
                    if let Err(e) = s.send_command(&cmd) {
                        app.set_status(format!("Terminal write failed: {e}"));
                    }
                    resp.request_focus();
                }
            });
        });
}

// helpers used by terminal_tab
impl TermSession {
    pub fn is_alive_silent(&mut self) -> bool {
        match &mut self.backend {
            Backend::Real(s) => !s.exited.load(Ordering::SeqCst),
            Backend::Legacy(s) => !s.child.try_wait().map(|o| o.is_some()).unwrap_or(true),
        }
    }
}
