//! GitHub integration — clone a repository and open it as a workspace.
//!
//! Strategy (100% offline-capable tools only, no new crates):
//!   1. If `git` is on PATH  -> `git clone --depth 1 <url> <dest>`
//!   2. Otherwise -> download the branch ZIP via GitHub codeload +
//!      `Expand-Archive`, then move the inner folder into place.
//! Both paths run inside a dedicated PowerShell child process so the UI stays
//! responsive, and the live log is streamed into the clone dialog.

use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui;

use crate::app::App;
use crate::theme::Theme;

// ---------------------------------------------------------------- job

pub struct CloneJob {
    child: Child,
    buf: Arc<Mutex<String>>,
    done: Arc<AtomicBool>,
}

impl CloneJob {
    fn spawn(script: &str, workdir: &std::path::Path) -> std::io::Result<Self> {
        let script_path = workdir.join("rea_clone.ps1");
        std::fs::write(&script_path, script)?;

        let mut cmd = Command::new("powershell.exe");
        cmd.args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&script_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = cmd.spawn()?;
        let stdout = child.stdout.take().expect("stdout");
        let stderr = child.stderr.take().expect("stderr");

        let buf: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let done = Arc::new(AtomicBool::new(false));

        for (stream, tag) in [(Box::new(stdout) as Box<dyn Read + Send>, ""), (Box::new(stderr) as Box<dyn Read + Send>, "")] {
            let buf = Arc::clone(&buf);
            let done = Arc::clone(&done);
            std::thread::spawn(move || {
                let mut r = BufReader::new(stream);
                let mut line = String::new();
                loop {
                    line.clear();
                    match r.read_line(&mut line) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            let mut b = buf.lock().unwrap();
                            b.push_str(&line);
                            if b.len() > 60_000 {
                                let cut = b.len() - 40_000;
                                // floor `cut` to a char boundary — the buffer may
                                // contain multi-byte UTF-8
                                let cut = (cut..=b.len())
                                    .find(|&i| b.is_char_boundary(i))
                                    .unwrap_or(b.len());
                                let idx = b[cut..].find('\n').map(|i| cut + i + 1).unwrap_or(cut);
                                b.drain(..idx);
                            }
                        }
                    }
                }
                let _ = tag;
                done.store(true, Ordering::SeqCst);
            });
        }

        Ok(Self { child, buf, done })
    }

    pub fn text(&self) -> String {
        self.buf.lock().unwrap().clone()
    }

    pub fn is_done(&self) -> bool {
        self.done.load(Ordering::SeqCst)
    }
}

impl Drop for CloneJob {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ---------------------------------------------------------------- url parsing

/// Accepts: `owner/repo`, `github.com/owner/repo`, `https://github.com/owner/repo(.git)(/tree/…)` ,
/// `git@github.com:owner/repo.git`. Returns `owner/repo` and repo name.
pub fn parse_repo(input: &str) -> Option<(String, String)> {
    let s = input.trim();
    let s = s.trim_end_matches('/');
    let after_domain = if let Some(p) = s.find("github.com") {
        let rest = &s[p + "github.com".len()..];
        let rest = rest.trim_start_matches(|c| c == ':' || c == '/');
        rest
    } else {
        // bare owner/repo form
        s
    };
    let mut parts: Vec<&str> = after_domain
        .trim_end_matches(".git")
        .split('/')
        .filter(|x| !x.is_empty())
        .collect();
    if parts.len() < 2 {
        return None;
    }
    parts.truncate(2);
    let owner = parts[0].to_string();
    let repo = parts[1].to_string();
    let valid = |x: &str| {
        !x.is_empty()
            && x.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    };
    if valid(&owner) && valid(&repo) {
        Some((format!("{owner}/{repo}"), repo))
    } else {
        None
    }
}

fn esc(s: &str) -> String {
    s.replace('\'', "''")
}

// ---------------------------------------------------------------- dialog state

#[derive(Default)]
pub struct GitHubDialog {
    pub url: String,
    pub dest_base: String,
    pub focus_url: bool,
}

pub fn default_dest_base() -> String {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    let mut p = std::path::PathBuf::from(home);
    p.push("REAProjects");
    p.display().to_string()
}

// ---------------------------------------------------------------- App methods

impl App {
    pub fn open_github_dialog(&mut self) {
        self.palette = None;
        self.suggest = None;
        let mut d: GitHubDialog = self.github.take().unwrap_or_default();
        if d.dest_base.is_empty() {
            d.dest_base = default_dest_base();
        }
        d.focus_url = true;
        self.github = Some(d);
    }

    pub fn browse_clone_dest(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .set_title("Choose where repositories are cloned")
            .pick_folder()
        {
            if let Some(d) = self.github.as_mut() {
                d.dest_base = p.display().to_string();
            }
        }
    }

    pub fn start_clone(&mut self) {
        let Some(d) = self.github.as_ref() else { return };
        let Some((repo, repo_name)) = parse_repo(&d.url) else {
            self.set_status("Invalid GitHub URL — use owner/repo or a github.com link");
            return;
        };
        let base = if d.dest_base.trim().is_empty() {
            default_dest_base()
        } else {
            d.dest_base.trim().to_string()
        };
        let dest = std::path::Path::new(&base).join(&repo_name);
        if let Err(e) = std::fs::create_dir_all(&base) {
            self.set_status(format!("Cannot create folder: {e}"));
            return;
        }
        let dest_s = dest.display().to_string();
        if std::path::Path::new(&dest_s).exists() {
            self.set_status(format!("Target already exists: {dest_s}"));
            // still allow re-clone: the script removes the zip-extracted path,
            // but for git we bail — show clear message
        }

        let script = format!(
            r#"$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$dest = '{dest}'
$repo = '{repo}'
$ok = $false
Write-Output "REA Studio clone: $repo"
Write-Output "Destination: $dest"
try {{
  if (Get-Command git -ErrorAction SilentlyContinue) {{
    Write-Output 'Using git (shallow clone)…'
    & git clone --depth 1 "https://github.com/$repo.git" "$dest" 2>&1 | ForEach-Object {{ Write-Output "$_" }}
    if ($LASTEXITCODE -eq 0 -and (Test-Path (Join-Path $dest '.git'))) {{ $ok = $true }}
  }} else {{
    Write-Output 'git not found — downloading ZIP from GitHub…'
    $api = Invoke-RestMethod -Uri "https://api.github.com/repos/$repo" -Headers {{ 'User-Agent' = 'REAStudio' }}
    $br = $api.default_branch
    Write-Output "Default branch: $br"
    $zip = Join-Path $env:TEMP 'rea_clone.zip'
    $url = "https://codeload.github.com/$repo/zip/refs/heads/$br"
    Write-Output "Downloading $url"
    Invoke-WebRequest -Uri $url -OutFile $zip -UserAgent 'REAStudio' -UseBasicParsing
    Write-Output 'Extracting…'
    $tmp = Join-Path $env:TEMP ('rea_tmp_' + [guid]::NewGuid().ToString('N'))
    if (Test-Path $dest) {{ Remove-Item $dest -Recurse -Force }}
    Expand-Archive -Path $zip -DestinationPath $tmp -Force
    $inner = Get-ChildItem -Path $tmp | Select-Object -First 1
    Move-Item -Path $inner.FullName -Destination $dest
    Remove-Item $tmp -Recurse -Force
    Remove-Item $zip -Force -ErrorAction SilentlyContinue
    $ok = $true
  }}
}} catch {{
  Write-Output ("ERROR: " + $_.Exception.Message)
}}
if ($ok) {{ Write-Output '__REA_CLONE_OK__' }} else {{ Write-Output '__REA_CLONE_FAIL__' }}
"#,
            dest = esc(&dest_s),
            repo = esc(&repo),
        );

        match CloneJob::spawn(&script, &self.cfg_dir) {
            Ok(job) => {
                self.clone_job = Some(job);
                self.set_status(format!("Cloning {repo}…"));
            }
            Err(e) => self.set_status(format!("Failed to start PowerShell: {e}")),
        }
    }

    pub fn poll_clone(&mut self) {
        let done = self
            .clone_job
            .as_ref()
            .map(|j| j.is_done())
            .unwrap_or(false);
        if !done {
            return;
        }
        let mut job = self.clone_job.take().unwrap();
        let _ = job.child.wait();
        let text = job.text();
        let ok = text.contains("__REA_CLONE_OK__");

        let Some(d) = self.github.as_ref() else { return };
        let repo = parse_repo(&d.url).map(|(r, _)| r).unwrap_or_default();
        let base = if d.dest_base.trim().is_empty() {
            default_dest_base()
        } else {
            d.dest_base.trim().to_string()
        };
        let dest = std::path::Path::new(&base)
            .join(repo.rsplit('/').next().unwrap_or("repo"));
        if ok {
            self.set_workspace(dest.clone());
            let cd = format!(
                "Set-Location -LiteralPath '{}'",
                dest.display().to_string().replace('\'', "''")
            );
            self.send_terminal(&cd);
            self.github = None;
            self.set_status(format!("Cloned into {}", dest.display()));
        } else {
            self.set_status("Clone failed — see the log in the dialog");
        }
    }
}

// ---------------------------------------------------------------- dialog UI

pub fn show(app: &mut App, ctx: &egui::Context, t: &Theme) {
    let Some(d) = app.github.as_mut() else { return };
    let busy = app.clone_job.is_some();

    // keep polling while busy
    if busy {
        ctx.request_repaint_after(std::time::Duration::from_millis(120));
    }

    let mut start = false;
    let mut cancel = false;
    let mut browse = false;

    egui::Window::new(
        egui::RichText::new("Clone from GitHub")
            .size(15.0)
            .strong()
            .color(t.text),
    )
    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
    .order(egui::Order::Tooltip)
    .collapsible(false)
    .resizable(false)
    .show(ctx, |ui| {
        ui.set_width(480.0);

        ui.label(
            egui::RichText::new(
                "Paste a GitHub link — the project is cloned and opened as your workspace.",
            )
            .size(11.5)
            .color(t.text_dim),
        );
        ui.add_space(8.0);

        // ---- url row ----
        ui.label(egui::RichText::new("Repository URL").size(11.0).color(t.text_faint));
        let mut url = d.url.clone();
        let resp = egui::TextEdit::singleline(&mut url)
            .hint_text("https://github.com/owner/repo  —  or  owner/repo")
            .desired_width(ui.available_width())
            .font(egui::TextStyle::Monospace)
            .show(ui);
        if resp.response.changed() {
            d.url = url;
        }
        if d.focus_url {
            resp.response.request_focus();
            d.focus_url = false;
        }

        // live parse feedback
        match parse_repo(&d.url) {
            Some((repo, _)) => {
                ui.label(
                    egui::RichText::new(format!("  OK  github.com/{repo}"))
                        .size(11.0)
                        .color(t.ok),
                );
            }
            None if !d.url.trim().is_empty() => {
                ui.label(
                    egui::RichText::new("  Not a valid GitHub repository link")
                        .size(11.0)
                        .color(t.err),
                );
            }
            _ => {}
        }
        ui.add_space(8.0);

        // ---- dest row ----
        ui.label(
            egui::RichText::new("Clone into (folder)")
                .size(11.0)
                .color(t.text_faint),
        );
        ui.horizontal(|ui| {
            let mut dest = d.dest_base.clone();
            let r2 = egui::TextEdit::singleline(&mut dest)
                .desired_width(ui.available_width() - 40.0)
                .font(egui::TextStyle::Monospace)
                .show(ui);
            if r2.response.changed() {
                d.dest_base = dest;
            }
            if ui
                .add(egui::Button::new(egui::RichText::new("…").size(13.0)))
                .clicked()
            {
                browse = true;
            }
        });
        if let Some((_, repo)) = parse_repo(&d.url) {
            let final_path = std::path::Path::new(&d.dest_base).join(&repo);
            ui.label(
                egui::RichText::new(format!("  {}", final_path.display()))
                    .size(10.5)
                    .color(t.text_faint),
            );
        }
        ui.add_space(10.0);

        // ---- actions ----
        if busy {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(
                    egui::RichText::new("Cloning…")
                        .size(13.0)
                        .strong()
                        .color(t.accent),
                );
            });
        } else {
            ui.horizontal(|ui| {
                let valid = parse_repo(&d.url).is_some();
                let b = egui::Button::new(
                    egui::RichText::new("Clone").size(13.0).strong(),
                );
                if ui
                    .add_enabled(valid, b)
                    .clicked()
                {
                    start = true;
                }
                if ui.add(egui::Button::new(egui::RichText::new("Cancel").size(13.0))).clicked() {
                    cancel = true;
                }
            });
        }

        // ---- log ----
        if let Some(job) = app.clone_job.as_ref() {
            ui.add_space(8.0);
            ui.separator();
            let text = job.text();
            egui::ScrollArea::vertical()
                .max_height(180.0)
                .auto_shrink([false, true])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(
                        egui::RichText::new(text)
                            .size(10.5)
                            .color(t.text_dim)
                            .family(egui::FontFamily::Monospace),
                    );
                });
        }
    });

    if browse {
        app.browse_clone_dest();
    }
    if cancel {
        app.github = None;
    }
    if start {
        app.start_clone();
    }

    // Esc closes when not busy
    if !busy {
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && !resp_focused(ctx) {
            app.github = None;
        }
    }
}

/// don't close on Esc while a text field is focused (let it clear focus first)
fn resp_focused(ctx: &egui::Context) -> bool {
    ctx.memory(|m| m.focused().is_some())
}
