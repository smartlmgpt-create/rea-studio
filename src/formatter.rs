//! v1.7 — one-click formatters (rustfmt / black / prettier / gofmt / clang-format).
//!
//! The formatter for a file is picked by extension and run as a filter:
//! the buffer text goes in on stdin, the formatted text comes back on
//! stdout. If the tool is not installed the file is left untouched and the
//! status bar explains what to install.

use std::collections::HashMap;
use std::process::{Command, Stdio};

use crate::app::App;

pub struct FormatTool {
    pub name: &'static str,
    pub install: &'static str,
    pub args: Vec<String>,
}

/// Pick a formatter command for a file extension / name.
pub fn tool_for(ext: &str, file_name: &str) -> Option<FormatTool> {
    let (name, install, mut args): (&str, &str, Vec<String>) = match ext {
        "rs" => (
            "rustfmt",
            "rustup component add rustfmt",
            vec!["--emit".into(), "stdout".into()],
        ),
        "py" => (
            "black",
            "pip install black",
            vec!["-q".into(), "-".into()],
        ),
        "go" => ("gofmt", "install Go (go.dev)", vec![]),
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" | "json" | "css" | "scss" | "html" | "htm"
        | "md" | "markdown" | "yml" | "yaml" | "vue" | "svelte" => (
            "prettier",
            "npm install -g prettier",
            vec![
                "--stdin-filepath".into(),
                file_name.to_string(),
            ],
        ),
        "c" | "h" | "cpp" | "hpp" | "cc" | "hh" | "cxx" | "ino" => (
            "clang-format",
            "install LLVM / clang-format",
            vec!["--assume-filename".into(), file_name.to_string()],
        ),
        "rb" => (
            "rubocop",
            "gem install rubocop",
            vec!["--stdin".into(), file_name.to_string(), "--autocorrect".into(), "--stderr".into()],
        ),
        _ => return None,
    };
    // rubocop writes the result to stdout but prints notices on stderr — keep
    args.retain(|a| a != "--stderr");
    Some(FormatTool { name, install, args })
}

/// Is this tool available on PATH? (cached per session)
pub fn available(cache: &mut HashMap<String, bool>, tool: &str) -> bool {
    if let Some(v) = cache.get(tool) {
        return *v;
    }
    let ok = Command::new(if cfg!(windows) { "where" } else { "which" })
        .arg(tool)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    cache.insert(tool.to_string(), ok);
    ok
}

/// Format the active document in place (with undo step).
pub fn format_active(app: &mut App) {
    let Some(i) = app.active else {
        app.set_status("Format: no file open");
        return;
    };
    let (ext, name, text) = {
        let d = &app.docs[i];
        (d.ext(), d.name.clone(), d.text.clone())
    };
    let Some(tool) = tool_for(&ext, &name) else {
        app.set_status(format!("Format: no formatter configured for .{ext}"));
        return;
    };
    if !available(&mut app.fmt_cache, tool.name) {
        app.set_status(format!(
            "Format: `{}` not found — {}",
            tool.name, tool.install
        ));
        return;
    }

    let mut child = Command::new(tool.name)
        .args(&tool.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    match child {
        Ok(mut c) => {
            use std::io::Write;
            // FIX (deadlock): writing the whole document to stdin BEFORE
            // reading stdout froze the app whenever the formatted output
            // exceeded the OS pipe buffer (child blocked on stdout while we
            // blocked on stdin). Write from a helper thread so the pipes
            // progress in parallel.
            let mut stdin = c.stdin.take();
            let text_for_child = text.clone();
            let writer = std::thread::spawn(move || {
                if let Some(si) = stdin.as_mut() {
                    let _ = si.write_all(text_for_child.as_bytes());
                }
                // closing the pipe gives the child EOF on stdin
            });
            let out = c.wait_with_output();
            let _ = writer.join();
            match out {
                Ok(out) if out.status.success() => {
                    let formatted = String::from_utf8_lossy(&out.stdout).into_owned();
                    if !formatted.trim().is_empty() && formatted != text {
                        let changed = formatted.clone();
                        app.set_doc_text_undo(i, changed, None);
                        app.set_status(format!("Formatted with {}", tool.name));
                    } else {
                        app.set_status(format!("Already formatted ({})", tool.name));
                    }
                }
                Ok(out) => {
                    let err = String::from_utf8_lossy(&out.stderr);
                    let first = err.lines().next().unwrap_or("format failed").to_string();
                    app.set_status(format!("{}: {first}", tool.name));
                }
                Err(e) => app.set_status(format!("{} failed: {e}", tool.name)),
            }
        }
        Err(e) => app.set_status(format!("{} failed to start: {e}", tool.name)),
    }
}

impl App {
    pub fn fmt_available(&mut self, tool: &str) -> bool {
        available(&mut self.fmt_cache, tool)
    }
}
