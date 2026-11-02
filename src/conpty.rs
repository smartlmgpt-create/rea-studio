//! v1.7 — real ConPTY pseudo-console (Windows 10 1809+).
//!
//! Spawns a shell attached to a Windows pseudo console so full-screen TUI
//! programs (vim, less, htop) render and interactive prompts (passwords,
//! y/n confirmations) work. Output arrives as VT sequences → `vt::Vt`.
//!
//! FFI follows the official ConPTY sample: anonymous pipes →
//! CreatePseudoConsole → attribute list (PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE)
//! → CreateProcessW(EXTENDED_STARTUPINFO_PRESENT).

#![cfg(windows)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::Console::{
    ClosePseudoConsole, CreatePseudoConsole, ResizePseudoConsole, COORD, HPCON,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
    InitializeProcThreadAttributeList, TerminateProcess, UpdateProcThreadAttribute,
    EXTENDED_STARTUPINFO_PRESENT, LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION,
    PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, STARTUPINFOEXW,
};

use crate::vt::Vt;
use crate::terminal::ShellKind;

// ---------------------------------------------------------------- utf8 helper

/// Assembles raw chunks into valid UTF-8 without breaking multi-byte chars
/// across read boundaries.
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

// ---------------------------------------------------------------- session

/// Raw HANDLE wrapper — raw pointers are !Send by default, but a pipe handle
/// value is just an integer; sending it to our own reader/writer threads is
/// exactly what the ConPTY sample does.
#[repr(transparent)]
struct SendHandle(usize);
unsafe impl Send for SendHandle {}


fn cmdline_for(kind: ShellKind) -> &'static str {
    match kind {
        ShellKind::PowerShell => "powershell.exe -NoLogo",
        ShellKind::Cmd => "cmd.exe",
        ShellKind::GitBash => "bash.exe -i -l",
    }
}

pub struct ConPtySession {
    pub vt: Vt,
    hpcon: HPCON,
    hproc: HANDLE,
    hthread: HANDLE,
    in_write: HANDLE,
    out_read: HANDLE,
    pub exited: Arc<AtomicBool>,
    pub exit_code: Arc<Mutex<u32>>,
    inbuf: Arc<Mutex<Vec<u8>>>,
    replies: Arc<Mutex<Vec<String>>>,
    raw_out: Arc<Mutex<String>>,
    pub cols: usize,
    pub rows: usize,
}

fn to_wide(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

impl ConPtySession {
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
            let size = COORD { X: cols.clamp(20, 500) as i16, Y: rows.clamp(6, 300) as i16 };

            // 1) pipes (inheritable)
            let mut sa = windows_sys::Win32::Security::SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<windows_sys::Win32::Security::SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: std::ptr::null_mut(),
                bInheritHandle: 1,
            };
            let mut in_read: HANDLE = std::ptr::null_mut();
            let mut in_write: HANDLE = std::ptr::null_mut();
            let mut out_read: HANDLE = std::ptr::null_mut();
            let mut out_write: HANDLE = std::ptr::null_mut();
            if CreatePipe(&mut in_read, &mut in_write, &mut sa, 0) == 0 {
                return Err("CreatePipe(in) failed".into());
            }
            if CreatePipe(&mut out_read, &mut out_write, &mut sa, 0) == 0 {
                return Err("CreatePipe(out) failed".into());
            }

            // 2) pseudo console
            let mut hpcon: HPCON = 0;
            let hr = CreatePseudoConsole(size, in_read, out_write, 0, &mut hpcon);
            if hr != 0 {
                return Err(format!(
                    "CreatePseudoConsole failed (HRESULT {hr:#x}) — Windows 10 1809+ required"
                ));
            }
            // our ends are now owned by the conpty
            CloseHandle(in_read);
            CloseHandle(out_write);

            // 3) attribute list
            let mut attr_size: usize = 0;
            let _ = InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut attr_size);
            let mut attr_buf = vec![0u8; attr_size];
            let attr_list = attr_buf.as_mut_ptr() as LPPROC_THREAD_ATTRIBUTE_LIST;
            if InitializeProcThreadAttributeList(attr_list, 1, 0, &mut attr_size) == 0 {
                return Err("InitializeProcThreadAttributeList failed".into());
            }
            if UpdateProcThreadAttribute(
                attr_list,
                0,
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                hpcon as *mut std::ffi::c_void,
                std::mem::size_of::<HPCON>(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ) == 0
            {
                return Err("UpdateProcThreadAttribute failed".into());
            }

            // 4) spawn
            let mut cmdline = to_wide(cmdline_for(shell));
            let mut siex: STARTUPINFOEXW = std::mem::zeroed();
            siex.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
            siex.lpAttributeList = attr_list;
            let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
            let cwd_w = cwd.map(|p| to_wide(&p.display().to_string()));

            let ok = CreateProcessW(
                std::ptr::null(),
                cmdline.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                EXTENDED_STARTUPINFO_PRESENT,
                std::ptr::null(),
                if let Some(v) = &cwd_w { v.as_ptr() } else { std::ptr::null() },
                &mut siex.StartupInfo,
                &mut pi,
            );
            DeleteProcThreadAttributeList(attr_list);
            if ok == 0 {
                let e = std::io::Error::last_os_error();
                ClosePseudoConsole(hpcon);
                return Err(format!("CreateProcess failed: {e}"));
            }

            let hproc = pi.hProcess;
            let hthread = pi.hThread;
            let exited = Arc::new(AtomicBool::new(false));
            let exit_code = Arc::new(Mutex::new(0u32));
            let inbuf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
            let replies: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
            let raw_out: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));

            // 5) reader thread: conpty output → raw_out (UI pumps into vt)
            {
                let raw_out = Arc::clone(&raw_out);
                let exited = Arc::clone(&exited);
                let exit_code = Arc::clone(&exit_code);
                let out_read = SendHandle(out_read as usize);
                let hproc = SendHandle(hproc as usize);
                std::thread::spawn(move || {
                    let out_read = out_read.0 as HANDLE;
                    let hproc = hproc.0 as HANDLE;
                    let mut asm = Utf8Asm::new();
                    let mut buf = [0u8; 8192];
                    loop {
                        let mut nread: u32 = 0;
                        let ok = windows_sys::Win32::Storage::FileSystem::ReadFile(
                            out_read,
                            buf.as_mut_ptr(),
                            buf.len() as u32,
                            &mut nread,
                            std::ptr::null_mut(),
                        );
                        if ok == 0 || nread == 0 {
                            break;
                        }
                        let s = asm.push(&buf[..nread as usize]);
                        if !s.is_empty() {
                            let mut g = raw_out.lock().unwrap();
                            // FIX: when the panel is hidden (other bottom tab
                            // open) nothing pumps raw_out, so it used to grow
                            // without bound while a process kept printing.
                            // Emergency cap: drop the backlog instead of OOM.
                            if g.len() > 4_000_000 {
                                g.clear();
                            }
                            g.push_str(&s);
                        }
                    }
                    let mut code: u32 = 0;
                    if GetExitCodeProcess(hproc, &mut code) != 0 {
                        *exit_code.lock().unwrap() = code;
                    }
                    exited.store(true, Ordering::SeqCst);
                });
            }

            // 6) writer thread: input bytes + DSR/DA replies → conpty input
            {
                let inbuf = Arc::clone(&inbuf);
                let replies = Arc::clone(&replies);
                let in_write = SendHandle(in_write as usize);
                std::thread::spawn(move || {
                    let in_write = in_write.0 as HANDLE;
                    loop {
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
                                reps.into_iter()
                                    .flat_map(|s| s.into_bytes())
                                    .collect()
                            } else {
                                taken
                            }
                        };
                        let mut written: u32 = 0;
                        let _ = windows_sys::Win32::Storage::FileSystem::WriteFile(
                            in_write,
                            batch.as_ptr(),
                            batch.len() as u32,
                            &mut written,
                            std::ptr::null_mut(),
                        );
                    }
                });
            }

            Ok(Self {
                vt: Vt::new(cols, rows, default_fg, default_bg),
                hpcon,
                hproc,
                hthread,
                in_write,
                out_read,
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

    /// Feed pending conpty output into the vt (called from the UI thread).
    pub fn pump(&mut self) {
        let s = std::mem::take(&mut *self.raw_out.lock().unwrap());
        if !s.is_empty() {
            self.vt.feed(&s);
        }
        // forward emulator replies (DSR/DA) into the input pipe
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
        unsafe {
            let _ = ResizePseudoConsole(self.hpcon, COORD { X: cols as i16, Y: rows as i16 });
        }
    }

    pub fn kill(&mut self) {
        unsafe {
            TerminateProcess(self.hproc, 1);
        }
    }
}

impl Drop for ConPtySession {
    fn drop(&mut self) {
        unsafe {
            TerminateProcess(self.hproc, 1);
            ClosePseudoConsole(self.hpcon);
            CloseHandle(self.hproc);
            CloseHandle(self.hthread);
            CloseHandle(self.in_write);
            CloseHandle(self.out_read);
        }
    }
}
