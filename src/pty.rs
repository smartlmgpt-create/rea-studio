//! v1.8 — POSIX PTY pseudo-terminal backend (Linux + macOS).
//!
//! The unix counterpart of `conpty::ConPtySession`: spawns a shell attached
//! to a real pseudo-terminal so full-screen TUI programs (vim, less, htop)
//! render through the same `vt::Vt` emulator, and interactive prompts
//! (passwords, y/n confirmations) work.
//!
//! The public API deliberately mirrors ConPtySession 1:1 (vt, exited,
//! exit_code, cols, rows, spawn, pump, write_input, resize, kill) so
//! `terminal.rs` can share its whole renderer and dispatch code through the
//! `RealPty` type alias.
//!
//! PTY dance (the portable way, like script(1)):
//!   posix_openpt(O_RDWR|O_NOCTTY) → grantpt → unlockpt → ptsname_r
//!   child: pre_exec { setsid(); open(slave); dup2 → 0/1/2 }  — first open
//!   by a session leader without a controlling tty auto-assigns it, so this
//!   works on both Linux and macOS without TIOCSCTTY.

#![cfg(unix)]

use std::process::{Child, Command, Stdio};
use std::os::unix::process::CommandExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::terminal::ShellKind;
use crate::vt::Vt;

// ---------------------------------------------------------------- utf8 helper

/// Assembles raw chunks into valid UTF-8 without breaking multi-byte chars
/// across read boundaries. (Same algorithm as conpty.rs.)
struct Utf8Asm {
    pend: Vec<u8>,
}

impl Utf8Asm {
    fn new() -> Self {
        Utf8Asm { pend: Vec::new() }
    }
    fn push(&mut self, chunk: &[u8]) -> String {
        self.pend.extend_from_slice(chunk);
        let mut ok_len = self.pend.len();
        match std::str::from_utf8(&self.pend) {
            Ok(_) => {}
            Err(e) => {
                ok_len = e.valid_up_to();
            }
        }
        let out = String::from_utf8_lossy(&self.pend[..ok_len]).into_owned();
        self.pend.drain(..ok_len);
        // drop hopeless bytes if the pending buffer grows unbounded
        if self.pend.len() > 16 {
            let out2 = String::from_utf8_lossy(&self.pend).into_owned();
            self.pend.clear();
            return format!("{out}{out2}");
        }
        out
    }
}

// ---------------------------------------------------------------- shell choice

/// Map the (windows-named) ShellKind entries to unix programs.
/// PowerShell → the user's $SHELL (tab label shows "shell"),
/// Cmd → bash, GitBash → a login bash (`bash -l`).
fn shell_program(kind: ShellKind) -> (String, Vec<String>) {
    match kind {
        ShellKind::GitBash => ("bash".to_string(), vec!["-l".to_string()]),
        ShellKind::Cmd => ("bash".to_string(), Vec::new()),
        ShellKind::PowerShell => {
            let sh = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
            (sh, Vec::new())
        }
    }
}

// ---------------------------------------------------------------- session

pub struct PtySession {
    pub vt: Vt,
    master: i32,
    child: Child,
    pub exited: Arc<AtomicBool>,
    pub exit_code: Arc<Mutex<u32>>,
    inbuf: Arc<Mutex<Vec<u8>>>,
    replies: Arc<Mutex<Vec<String>>>,
    raw_out: Arc<Mutex<String>>,
    pub cols: usize,
    pub rows: usize,
}

/// Best-effort raw fd close (used on early-error paths only).
fn close_fd(fd: i32) {
    unsafe {
        libc::close(fd);
    }
}

impl PtySession {
    /// Spawn `shell` in `cwd` with the given grid size.
    pub fn spawn(
        shell: ShellKind,
        cwd: Option<&std::path::Path>,
        cols: usize,
        rows: usize,
        default_fg: eframe::egui::Color32,
        default_bg: eframe::egui::Color32,
    ) -> Result<Self, String> {
        unsafe {
            // 1) open the PTY master
            let master = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
            if master < 0 {
                return Err("posix_openpt failed".into());
            }
            if libc::grantpt(master) != 0 {
                close_fd(master);
                return Err("grantpt failed".into());
            }
            if libc::unlockpt(master) != 0 {
                close_fd(master);
                return Err("unlockpt failed".into());
            }

            // 2) resolve the slave device path — ptsname_r on Linux,
            // ptsname elsewhere (macOS libc crate lacks the _r variant;
            // this is our own master fd, no other thread names it yet)
            let slave_c: std::ffi::CString = {
                #[cfg(target_os = "linux")]
                {
                    let mut namebuf = [0u8; 256];
                    if libc::ptsname_r(master, namebuf.as_mut_ptr().cast(), namebuf.len()) != 0 {
                        close_fd(master);
                        return Err("ptsname failed".into());
                    }
                    let name_end =
                        namebuf.iter().position(|&b| b == 0).unwrap_or(namebuf.len());
                    match std::ffi::CString::new(&namebuf[..name_end]) {
                        Ok(c) => c,
                        Err(e) => {
                            close_fd(master);
                            return Err(format!("slave path: {e}"));
                        }
                    }
                }
                #[cfg(not(target_os = "linux"))]
                {
                    let p = libc::ptsname(master);
                    if p.is_null() {
                        close_fd(master);
                        return Err("ptsname failed".into());
                    }
                    std::ffi::CStr::from_ptr(p).to_owned()
                }
            };

            // 3) initial grid size (kernel forwards SIGWINCH on changes)
            let _ = set_pty_size(master, cols, rows);

            // 4) spawn the shell wired to the slave as its controlling tty
            let (prog, args) = shell_program(shell);
            let mut cmd = Command::new(&prog);
            cmd.args(&args);
            if let Some(d) = cwd {
                if d.exists() {
                    cmd.current_dir(d);
                }
            }
            let slave_c2 = slave_c.clone();
            unsafe {
                // SAFETY: runs in the forked child before exec; only async-
                // signal-safe libc calls are used (setsid, open, dup2, close).
                cmd.pre_exec(move || {
                    libc::setsid(); // new session, detach from any ctty
                    let fd = libc::open(slave_c2.as_ptr(), libc::O_RDWR);
                    if fd < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    // session leader + first tty open → becomes controlling tty
                    #[cfg(target_os = "linux")]
                    {
                        libc::ioctl(fd, libc::TIOCSCTTY, 0);
                    }
                    libc::dup2(fd, 0);
                    libc::dup2(fd, 1);
                    libc::dup2(fd, 2);
                    if fd > 2 {
                        libc::close(fd);
                    }
                    Ok(())
                });
            }
            // real fds are wired inside pre_exec; give std something valid
            cmd.stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            let child = cmd
                .spawn()
                .map_err(|e| {
                    close_fd(master);
                    format!("{prog}: {e}")
                })?;
            let pid = child.id();

            let exited = Arc::new(AtomicBool::new(false));
            let exit_code = Arc::new(Mutex::new(0u32));
            let inbuf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
            let replies: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
            let raw_out: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));

            // 5) reader thread: master → raw_out (UI pumps into vt)
            {
                let raw_out = Arc::clone(&raw_out);
                let exited = Arc::clone(&exited);
                let exit_code = Arc::clone(&exit_code);
                std::thread::spawn(move || {
                    let mut asm = Utf8Asm::new();
                    let mut buf = [0u8; 8192];
                    loop {
                        let n = libc::read(master, buf.as_mut_ptr().cast(), buf.len());
                        if n <= 0 {
                            break; // EOF or error — shell closed its tty
                        }
                        let s = asm.push(&buf[..n as usize]);
                        if !s.is_empty() {
                            let mut g = raw_out.lock().unwrap();
                            // emergency cap: drop the backlog instead of OOM
                            if g.len() > 4_000_000 {
                                g.clear();
                            }
                            g.push_str(&s);
                        }
                    }
                    // reap the child and publish its exit code
                    let mut status: i32 = 0;
                    let code = unsafe {
                        if libc::waitpid(pid as i32, &mut status, 0) == pid as i32 {
                            if libc::WIFEXITED(status) {
                                libc::WEXITSTATUS(status) as u32
                            } else {
                                1
                            }
                        } else {
                            1
                        }
                    };
                    *exit_code.lock().unwrap() = code;
                    exited.store(true, Ordering::SeqCst);
                });
            }

            // 6) writer thread: input bytes + DSR/DA replies → master
            {
                let inbuf = Arc::clone(&inbuf);
                let replies = Arc::clone(&replies);
                std::thread::spawn(move || loop {
                    let batch: Vec<u8> = {
                        let mut b = inbuf.lock().unwrap();
                        let taken = std::mem::take(&mut *b);
                        if taken.is_empty() {
                            let mut r = replies.lock().unwrap();
                            let reps = std::mem::take(&mut *r);
                            if reps.is_empty() {
                                drop(r);
                                std::thread::sleep(std::time::Duration::from_millis(8));
                                continue;
                            }
                            drop(r);
                            reps.into_iter().flat_map(|s| s.into_bytes()).collect()
                        } else {
                            taken
                        }
                    };
                    let n = unsafe { libc::write(master, batch.as_ptr().cast(), batch.len()) };
                    if n < 0 {
                        break; // master closed — nothing to write to anymore
                    }
                });
            }

            Ok(Self {
                vt: Vt::new(cols, rows, default_fg, default_bg),
                master,
                child,
                exited,
                exit_code,
                inbuf,
                replies,
                raw_out,
                cols,
                rows,
            })
        }
    }

    /// Feed pending pty output into the vt (called from the UI thread).
    pub fn pump(&mut self) {
        let s = std::mem::take(&mut *self.raw_out.lock().unwrap());
        if !s.is_empty() {
            self.vt.feed(&s);
        }
        // forward emulator replies (DSR/DA) into the input side
        let reps = std::mem::take(&mut self.vt.replies);
        if !reps.is_empty() {
            self.replies.lock().unwrap().extend(reps);
        }
    }

    pub fn write_input(&self, s: &str) {
        self.inbuf.lock().unwrap().extend_from_slice(s.as_bytes());
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        let cols = cols.clamp(20, 500);
        let rows = rows.clamp(6, 300);
        if cols == self.cols && rows == self.rows {
            return;
        }
        self.cols = cols;
        self.rows = rows;
        self.vt.resize(cols, rows);
        let _ = set_pty_size(self.master, cols, rows);
    }

    pub fn kill(&mut self) {
        let _ = self.child.kill();
    }
}

/// ioctl(master, TIOCSWINSZ) — the kernel delivers SIGWINCH to the
/// foreground process group of the slave.
fn set_pty_size(master: i32, cols: usize, rows: usize) -> i32 {
    let ws = libc::winsize {
        ws_col: cols as u16,
        ws_row: rows as u16,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    unsafe { libc::ioctl(master, libc::TIOCSWINSZ, &ws) }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        // the reader thread's blocking waitpid reaps the child
        unsafe {
            libc::close(self.master);
        }
    }
}

// ---------------------------------------------------------------- tests

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn vt_text(vt: &Vt) -> String {
        let mut out = String::new();
        for r in 0..vt.rows {
            for c in vt.row(r) {
                out.push(c.ch);
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn pty_echo_roundtrip() {
        let mut s = PtySession::spawn(
            ShellKind::Cmd,
            None,
            80,
            24,
            eframe::egui::Color32::WHITE,
            eframe::egui::Color32::BLACK,
        )
        .expect("spawn pty");
        // give the shell a moment, then run an echo through the pty
        s.write_input("echo REA_PTY_OK\r");
        let mut text = String::new();
        for _ in 0..120 {
            s.pump();
            text = vt_text(&s.vt);
            if text.contains("REA_PTY_OK") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(text.contains("REA_PTY_OK"), "pty output: {text:?}");
        // resize must not panic or kill the session
        s.resize(100, 30);
        assert!(s.cols == 100 && s.rows == 30);
        s.kill();
    }
}
