//! v1.7 — GIT panel (bottom panel tab).
//!
//! A small source-control panel: branch name, staged/unstaged file lists
//! (git status --porcelain), per-file stage/unstage, stage all, commit with
//! message, push, pull and a colored diff view for the selected file.
//! git.exe must be on PATH (the panel says so otherwise).

use std::process::Command;
use std::sync::{Arc, Mutex};

use eframe::egui;
use egui::{vec2, Color32};

use crate::app::App;
use crate::theme::Theme;

#[derive(Clone)]
pub struct GitFile {
    pub x: char,          // index (staged) status
    pub y: char,          // worktree status
    pub path: String,
    pub orig_path: Option<String>, // renames "old -> new"
}

#[derive(Default)]
pub struct GitState {
    pub branch: String,
    pub staged: Vec<GitFile>,
    pub unstaged: Vec<GitFile>,
    pub untracked: Vec<GitFile>,
    pub msg: String,
    pub log: String,
    pub err: Option<String>,
    pub no_git: bool,
    /// background job result (push/pull/commit)
    pub job: Option<Arc<Mutex<Option<(bool, String)>>>>,
    pub job_busy: bool,
    /// file whose diff is shown
    pub diff_sel: Option<(String, bool)>, // (path, staged?)
    pub diff_text: String,
    pub need_refresh: bool,
    pub commit_in_progress: bool,
    /// path awaiting a second click on "discard" (one click must NOT wipe
    /// uncommitted work)
    pub discard_pending: Option<String>,
}

// ---------------------------------------------------------------- plumbing

fn git_args(args: &[&str], cwd: &std::path::Path) -> std::io::Result<std::process::Child> {
    let mut c = Command::new("git");
    c.args(args).current_dir(cwd);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    c.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    // no .expect() here — a missing git.exe must surface as a friendly
    // "install Git for Windows" message, not kill the whole editor
}

/// Run git synchronously, return (ok, combined output).
pub fn run_git(app: &App, args: &[&str]) -> (bool, String) {
    let Some(cwd) = app.workspace.clone() else {
        return (false, "no workspace folder open".into());
    };
    match git_args(args, &cwd).and_then(|c| c.wait_with_output()) {
        Ok(out) => {
            let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
            let err = String::from_utf8_lossy(&out.stderr).into_owned();
            if !err.trim().is_empty() {
                s.push_str(&err);
            }
            (out.status.success(), s)
        }
        Err(e) => {
            let msg = e.to_string();
            (
                false,
                if msg.contains("not found") || msg.contains("(os error 2)") {
                    "git was not found — install Git for Windows (git-scm.com)".into()
                } else {
                    msg
                },
            )
        }
    }
}

fn parse_status(out: &str) -> (String, Vec<GitFile>, Vec<GitFile>, Vec<GitFile>) {
    let mut branch = String::new();
    let mut staged = Vec::new();
    let mut unstaged = Vec::new();
    let mut untracked = Vec::new();
    for line in out.lines() {
        if let Some(b) = line.strip_prefix("## ") {
            branch = b
                .split("...")
                .next()
                .unwrap_or(b)
                .trim_start_matches('(')
                .trim()
                .to_string();
            continue;
        }
        if line.len() < 4 {
            continue;
        }
        let (xy, path) = line.split_at(2);
        let mut x = xy.chars().next().unwrap_or(' ');
        let y = xy.chars().nth(1).unwrap_or(' ');
        let mut path = path.trim_start().to_string();
        let mut orig = None;
        if let Some((a, b)) = path.split_once(" -> ") {
            orig = Some(a.to_string());
            path = b.to_string();
        }
        if x == '?' {
            untracked.push(GitFile { x: '?', y, path, orig_path: orig });
            continue;
        }
        if x == '!' {
            continue; // ignored
        }
        if x != ' ' {
            staged.push(GitFile { x, y, path: path.clone(), orig_path: orig.clone() });
        }
        if y != ' ' {
            if x == 'U' || x == 'A' {
                // newly added + modified again: still show in unstaged too
            }
            unstaged.push(GitFile { x, y, path, orig_path: orig });
        }
        let _ = x;
    }
    (branch, staged, unstaged, untracked)
}

// ---------------------------------------------------------------- refresh

pub fn refresh(app: &mut App) {
    if app.workspace.is_none() {
        return;
    }
    let (ok, out) = run_git(app, &["status", "--porcelain=v1", "-b"]);
    if !ok && out.contains("not found") {
        app.git_state.no_git = true;
        app.git_state.err = Some(out);
        return;
    }
    app.git_state.no_git = false;
    if !ok {
        // not a repo (or another error) — show hint
        app.git_state.err = Some(if out.contains("not a git repository") {
            "This folder is not a git repository.".into()
        } else {
            out
        });
        app.git_state.staged.clear();
        app.git_state.unstaged.clear();
        app.git_state.untracked.clear();
        return;
    }
    app.git_state.err = None;
    let (branch, staged, unstaged, untracked) = parse_status(&out);
    // clear diff if the file no longer exists in the lists
    if let Some((p, _)) = &app.git_state.diff_sel {
        let still = staged.iter().chain(&unstaged).chain(&untracked).any(|f| &f.path == p);
        if !still {
            app.git_state.diff_sel = None;
            app.git_state.diff_text.clear();
        }
    }
}

impl App {
    pub fn git_refresh(&mut self) {
        self.git_state.need_refresh = false;
        refresh(self);
    }

    pub fn git_show_diff(&mut self, path: String, staged: bool) {
        let args: Vec<&str> = if staged {
            vec!["diff", "--cached", "--", &path]
        } else {
            vec!["diff", "--", &path]
        };
        let (ok, out) = run_git(self, &args);
        self.git_state.diff_sel = Some((path, staged));
        self.git_state.diff_text = if ok && out.trim().is_empty() {
            "(no textual diff)".into()
        } else if ok {
            out
        } else {
            out
        };
    }

    pub fn git_stage(&mut self, path: &str) {
        let (_, out) = run_git(self, &["add", "--", path]);
        self.git_state.log = format!("git add {path}\n{out}");
        self.git_refresh();
    }

    pub fn git_unstage(&mut self, path: &str) {
        let (_, out) = run_git(self, &["reset", "HEAD", "--", path]);
        self.git_state.log = format!("git reset {path}\n{out}");
        self.git_refresh();
    }

    pub fn git_stage_all(&mut self) {
        let (_, out) = run_git(self, &["add", "-A"]);
        self.git_state.log = format!("git add -A\n{out}");
        self.git_refresh();
    }

    pub fn git_discard(&mut self, path: &str) {
        let (_, out) = run_git(self, &["checkout", "--", path]);
        self.git_state.log = format!("git checkout -- {path}\n{out}");
        self.git_refresh();
    }

    pub fn git_commit(&mut self) {
        let msg = self.git_state.msg.trim().to_string();
        if msg.is_empty() {
            self.git_state.log = "Commit message is empty.".into();
            return;
        }
        self.git_state.commit_in_progress = true;
        let job: Arc<Mutex<Option<(bool, String)>>> = Arc::new(Mutex::new(None));
        self.git_state.job = Some(Arc::clone(&job));
        self.git_state.job_busy = true;
        let cwd = self.workspace.clone().unwrap_or_default();
        let thread_msg = msg.clone();
        std::thread::spawn(move || {
            let mut c = Command::new("git");
            c.args(["commit", "-m", &thread_msg]).current_dir(&cwd);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                c.creation_flags(0x0800_0000);
            }
            let res = match c.output() {
                Ok(o) => {
                    let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
                    s.push_str(&String::from_utf8_lossy(&o.stderr));
                    (o.status.success(), s)
                }
                Err(e) => (false, e.to_string()),
            };
            *job.lock().unwrap() = Some(res);
        });
    }

    pub fn git_push_pull(&mut self, push: bool) {
        let job: Arc<Mutex<Option<(bool, String)>>> = Arc::new(Mutex::new(None));
        self.git_state.job = Some(Arc::clone(&job));
        self.git_state.job_busy = true;
        let cwd = self.workspace.clone().unwrap_or_default();
        std::thread::spawn(move || {
            let args: &[&str] = if push { &["push"] } else { &["pull"] };
            let mut c = Command::new("git");
            c.args(args).current_dir(&cwd);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                c.creation_flags(0x0800_0000);
            }
            let res = match c.output() {
                Ok(o) => {
                    let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
                    s.push_str(&String::from_utf8_lossy(&o.stderr));
                    (o.status.success(), s)
                }
                Err(e) => (false, e.to_string()),
            };
            *job.lock().unwrap() = Some(res);
        });
    }

    /// Called from update(): collect finished git jobs.
    pub fn git_poll(&mut self) {
        if !self.git_state.job_busy {
            return;
        }
        let done = self
            .git_state
            .job
            .as_ref()
            .and_then(|j| j.lock().unwrap().take());
        if let Some((ok, out)) = done {
            self.git_state.job_busy = false;
            self.git_state.commit_in_progress = false;
            self.git_state.log = out.clone();
            if ok {
                self.git_state.msg.clear();
            }
            self.git_refresh();
        }
    }
}

// ---------------------------------------------------------------- UI

pub fn show(app: &mut App, ui: &mut egui::Ui, t: &Theme) {
    if app.workspace.is_none() {
        ui.vertical_centered(|ui| {
            ui.add_space(14.0);
            ui.label(egui::RichText::new("⑂  Git").size(15.0).strong().color(t.text_dim));
            ui.label(
                egui::RichText::new("Open a folder (Ctrl+Shift+O) to use the Git panel.")
                    .size(12.0)
                    .color(t.text_faint),
            );
        });
        return;
    }
    if app.git_state.no_git {
        ui.vertical_centered(|ui| {
            ui.add_space(14.0);
            ui.label(egui::RichText::new("git was not found").size(13.0).color(t.err));
            ui.label(
                egui::RichText::new("Install Git for Windows from git-scm.com, then restart REA Studio.")
                    .size(12.0)
                    .color(t.text_faint),
            );
        });
        return;
    }

    // header: branch + refresh
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("⑂ {}", app.git_state.branch))
                .size(12.0)
                .strong()
                .color(t.accent),
        );
        if ui.button(egui::RichText::new("⟳ Refresh").size(10.5)).clicked() {
            app.git_refresh();
        }
        if ui.button(egui::RichText::new("Pull").size(10.5)).clicked() {
            app.git_push_pull(false);
        }
        if ui.button(egui::RichText::new("Push").size(10.5)).clicked() {
            app.git_push_pull(true);
        }
        if app.git_state.job_busy {
            ui.label(egui::RichText::new("● working…").size(10.5).color(t.warn));
        }
    });
    ui.separator();

    if let Some(err) = &app.git_state.err {
        ui.label(egui::RichText::new(err.clone()).size(11.0).color(t.warn));
        ui.add_space(4.0);
    }

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let width = ui.available_width();

            // ---- staged ----
            ui.label(egui::RichText::new(format!("STAGED ({})", app.git_state.staged.len())).size(10.0).strong().color(t.text_dim));
            let staged = app.git_state.staged.clone();
            for f in &staged {
                file_row(ui, app, t, f, true, width - 20.0);
            }
            if !staged.is_empty() {
                if ui.button(egui::RichText::new("▼ Unstage all").size(10.0)).clicked() {
                    for f in &staged {
                        app.git_unstage(&f.path);
                    }
                }
            }
            ui.add_space(4.0);

            // ---- unstaged ----
            ui.label(egui::RichText::new(format!("CHANGES ({})", app.git_state.unstaged.len())).size(10.0).strong().color(t.text_dim));
            let unstaged = app.git_state.unstaged.clone();
            for f in &unstaged {
                file_row(ui, app, t, f, false, width - 20.0);
            }
            // ---- untracked ----
            if !app.git_state.untracked.is_empty() {
                ui.label(egui::RichText::new(format!("UNTRACKED ({})", app.git_state.untracked.len())).size(10.0).strong().color(t.text_dim));
                let untracked = app.git_state.untracked.clone();
                for f in &untracked {
                    file_row(ui, app, t, f, false, width - 20.0);
                }
            }
            if !unstaged.is_empty() || !app.git_state.untracked.is_empty() {
                if ui.button(egui::RichText::new("▲ Stage all").size(10.0)).clicked() {
                    app.git_stage_all();
                }
            }

            // ---- commit ----
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let msg = &mut app.git_state.msg;
                let resp = ui.add(
                    egui::TextEdit::singleline(msg)
                        .hint_text("Commit message…")
                        .desired_width(width - 110.0),
                );
                let enabled = !app.git_state.staged.is_empty() && !app.git_state.commit_in_progress;
                if ui.add_enabled(enabled, egui::Button::new(egui::RichText::new("✓ Commit").size(11.0).strong())).clicked() {
                    app.git_commit();
                }
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && enabled {
                    app.git_commit();
                }
            });

            // ---- diff ----
            if let Some((p, staged)) = app.git_state.diff_sel.clone() {
                ui.add_space(6.0);
                ui.separator();
                ui.label(
                    egui::RichText::new(format!("DIFF — {}{}", p, if staged { " (staged)" } else { "" }))
                        .size(10.0)
                        .strong()
                        .color(t.text_dim),
                );
                let mut chars = app.git_state.diff_text.lines();
                egui::ScrollArea::vertical()
                    .id_salt("rea_git_diff")
                    .max_height(260.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                        for line in chars.by_ref() {
                            let col = if line.starts_with('+') {
                                Color32::from_rgb(0x67, 0xE2, 0x67)
                            } else if line.starts_with('-') {
                                Color32::from_rgb(0xF1, 0x4C, 0x4C)
                            } else if line.starts_with("@@") {
                                Color32::from_rgb(0x6F, 0xB3, 0xEF)
                            } else {
                                t.text_faint
                            };
                            ui.label(
                                egui::RichText::new(line)
                                    .font(egui::FontId::monospace(10.5))
                                    .color(col),
                            );
                        }
                    });
            }

            // ---- log ----
            if !app.git_state.log.trim().is_empty() {
                ui.add_space(6.0);
                ui.separator();
                ui.label(egui::RichText::new("OUTPUT").size(10.0).strong().color(t.text_dim));
                ui.label(
                    egui::RichText::new(app.git_state.log.trim_end().to_string())
                        .font(egui::FontId::monospace(10.5))
                        .color(t.text_faint),
                );
            }
        });
}

fn file_row(ui: &mut egui::Ui, app: &mut App, t: &Theme, f: &GitFile, staged: bool, width: f32) {
    let (badge, col) = match (f.x, f.y) {
        ('?', _) | ('A', _) => ("A", t.ok),
        ('M', 'M') | ('M', _) => ("M", t.warn),
        ('D', _) => ("D", t.err),
        ('R', _) => ("R", t.accent),
        ('U', _) => ("U", t.err),
        _ => ("M", t.warn),
    };
    egui::Frame::default()
        .fill(t.panel2)
        .rounding(egui::Rounding::same(5.0))
        .inner_margin(egui::Margin::symmetric(6.0, 2.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(badge).size(10.5).strong().color(col));
                let name = f.orig_path.as_ref().map(|o| format!("{o} → {}", f.path)).unwrap_or_else(|| f.path.clone());
                let resp = ui.add(
                    egui::Button::new(
                        egui::RichText::new(name).size(10.5).color(t.text),
                    )
                    .fill(Color32::TRANSPARENT)
                    .stroke(egui::Stroke::NONE),
                );
                if resp.clicked() {
                    app.git_show_diff(f.path.clone(), staged);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if staged {
                        if ui.button(egui::RichText::new("−").size(10.5)).on_hover_text("Unstage").clicked() {
                            app.git_unstage(&f.path);
                        }
                    } else {
                        if ui.button(egui::RichText::new("+").size(10.5)).on_hover_text("Stage").clicked() {
                            app.git_stage(&f.path);
                        }
                        // destructive: first click arms, second click confirms
                        if f.y != '?' && ui.button(egui::RichText::new(if app.git_state.discard_pending.as_deref() == Some(f.path.as_str()) { "sure?" } else { "↺" }).size(10.5)).on_hover_text("Discard changes").clicked() {
                            if app.git_state.discard_pending.as_deref() == Some(f.path.as_str()) {
                                app.git_state.discard_pending = None;
                                app.git_discard(&f.path);
                            } else {
                                app.git_state.discard_pending = Some(f.path.clone());
                            }
                        }
                    }
                });
            });
        });
    ui.add_space(2.0);
    let _ = width;
}
