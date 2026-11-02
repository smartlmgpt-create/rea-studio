//! v2.1: workspace TODO scanner — walks the open folder in a background
//! thread and lists every TODO / FIXME / XXX / HACK marker with its file and
//! line; clicking a hit opens the file and jumps to the line.
//!
//! The heavy walking happens OFF the UI thread (results come back over an
//! mpsc channel and are picked up by `poll()` from the frame loop).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::app::App;

pub struct TodoHit {
    pub path: PathBuf,
    /// 0-based line number
    pub line: usize,
    pub tag: String,
    pub text: String,
}

#[derive(Default)]
pub struct TodoState {
    pub hits: Vec<TodoHit>,
    pub running: bool,
    /// number of text files scanned in the last run
    pub files: usize,
}

const SKIP_DIRS: &[&str] = &[
    ".git", ".hg", ".svn", "node_modules", "target", "dist", "build", "out",
    "__pycache__", ".venv", "venv", "env", "vendor", ".idea", ".vscode",
    ".gradle", "bin", "obj", "Pods", "DerivedData", ".next", ".cache",
    "coverage", ".pytest_cache", "site-packages",
];

const MAX_DEPTH: usize = 12;
const MAX_FILES: usize = 6000;
const MAX_HITS: usize = 5000;
const MAX_FILE: u64 = 512 * 1024;

fn ext_ok(name: &str) -> bool {
    const EXT: &[&str] = &[
        "rs", "js", "mjs", "cjs", "ts", "tsx", "jsx", "py", "pyw", "go", "c", "h", "cpp", "hpp",
        "cc", "cxx", "java", "kt", "kts", "rb", "php", "cs", "lua", "md", "markdown", "txt",
        "html", "htm", "css", "scss", "less", "json", "yaml", "yml", "toml", "xml", "sh", "bash",
        "zsh", "bat", "cmd", "ps1", "sql", "vue", "svelte", "swift", "m", "mm", "pl", "pm",
        "zig", "v", "hs", "erl", "hrl", "ex", "exs", "clj", "cljs", "scala", "dart", "f90",
        "f95", "asm", "s", "r", "jl", "d", "ini", "cfg", "conf", "proto", "graphql", "prisma",
        "dockerfile", "makefile", "cmake", "gradle", "gitignore", "env",
    ];
    match name.rsplit_once('.') {
        Some((_, e)) => EXT.contains(&e.to_ascii_lowercase().as_str()),
        None => matches!(
            name,
            "Makefile" | "Dockerfile" | "CMakeLists.txt" | "Rakefile" | "Gemfile" | "Justfile"
        ),
    }
}

fn regex() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"\b(TODO|FIXME|XXX|HACK)\b").unwrap())
}

fn scan_dir(dir: &Path, out: &mut Vec<TodoHit>, files: &mut usize, depth: usize) {
    if depth > MAX_DEPTH || *files >= MAX_FILES || out.len() >= MAX_HITS {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let Ok(ft) = e.file_type() else { continue };
        let name = e.file_name().to_string_lossy().to_string();
        let path = e.path();
        if ft.is_dir() {
            if name.starts_with('.') || SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }
            scan_dir(&path, out, files, depth + 1);
            if *files >= MAX_FILES || out.len() >= MAX_HITS {
                return;
            }
        } else if ft.is_file() && ext_ok(&name) {
            if let Ok(md) = e.metadata() {
                if md.len() > MAX_FILE {
                    continue;
                }
            }
            *files += 1;
            scan_file(&path, out);
            if out.len() >= MAX_HITS {
                return;
            }
        }
    }
}

fn scan_file(path: &Path, out: &mut Vec<TodoHit>) {
    // cheap binary guard: a NUL in the first 4 KB means "not text"
    let Ok(bytes) = std::fs::read(path) else { return };
    if bytes.is_empty() {
        return;
    }
    if bytes[..bytes.len().min(4096)].contains(&0) {
        return;
    }
    let text = String::from_utf8_lossy(&bytes);
    let re = regex();
    for (i, line) in text.lines().enumerate() {
        if let Some(m) = re.find(line) {
            out.push(TodoHit {
                path: path.to_path_buf(),
                line: i,
                tag: m.as_str().to_string(),
                text: line.trim().chars().take(160).collect(),
            });
            if out.len() >= MAX_HITS {
                return;
            }
        }
    }
}

// ---------------------------------------------------------------- app glue

/// Start a background scan of the open workspace (no-op without a folder).
pub fn scan_start(app: &mut App) {
    let Some(ws) = app.workspace.clone() else {
        app.set_status("Open a folder first to scan for TODOs");
        return;
    };
    if app.todos.running {
        return;
    }
    app.todos = TodoState { hits: Vec::new(), running: true, files: 0 };
    let (tx, rx) = std::sync::mpsc::channel::<(Vec<TodoHit>, usize)>();
    std::thread::spawn(move || {
        let mut hits: Vec<TodoHit> = Vec::new();
        let mut files = 0usize;
        scan_dir(&ws, &mut hits, &mut files, 0);
        let _ = tx.send((hits, files));
    });
    app.todo_rx = Some(rx);
    app.set_status("Scanning workspace for TODO / FIXME…");
}

/// Frame-loop pickup of finished scans.
pub fn poll(app: &mut App) {
    if app.todo_rx.is_none() {
        return;
    }
    let done = app
        .todo_rx
        .as_ref()
        .and_then(|rx| rx.try_recv().ok());
    if let Some((hits, files)) = done {
        app.todo_rx = None;
        let n = hits.len();
        let nf = hits.iter().map(|h| h.path.clone()).collect::<std::collections::HashSet<_>>().len();
        app.todos = TodoState { hits, running: false, files };
        app.set_status(format!(
            "TODO scan: {} marker(s) in {} file(s), {} file(s) scanned",
            n, nf, files
        ));
    }
}

// ---------------------------------------------------------------- UI

/// Sidebar panel body (called from bars::show_sidebar).
pub fn ui(app: &mut App, ui: &mut egui::Ui, t: &crate::theme::Theme) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(egui::RichText::new("TODO Scanner").size(13.0).color(t.text));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(8.0);
            let label = if app.todos.running {
                "⏳ Scanning…"
            } else {
                "↻ Scan"
            };
            if ui
                .add(egui::Button::new(egui::RichText::new(label).size(11.5)))
                .clicked()
                && !app.todos.running
            {
                scan_start(app);
            }
        });
    });
    ui.separator();

    if app.todos.hits.is_empty() {
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(if app.todos.running {
                "Scanning workspace…"
            } else if app.todos.files > 0 {
                "No TODO / FIXME / XXX / HACK markers found."
            } else {
                "Press Scan to list every TODO / FIXME / XXX /\nHACK in the open folder.\n\nClick a hit to open it at its line."
            })
            .size(11.5)
            .color(t.text_dim),
        );
        return;
    }

    // summary chips
    ui.horizontal(|ui| {
        let mut count = |ui: &mut egui::Ui, tag: &str, col: egui::Color32| {
            let n = app.todos.hits.iter().filter(|h| h.tag == tag).count();
            if n > 0 {
                ui.label(egui::RichText::new(format!("{} {}", tag, n)).size(10.5).color(col));
            }
        };
        count(ui, "FIXME", t.err);
        count(ui, "TODO", t.warn);
        count(ui, "XXX", t.accent);
        count(ui, "HACK", t.accent);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(format!("{} files", app.todos.files))
                    .size(10.0)
                    .color(t.text_faint),
            );
        });
    });
    ui.separator();

    let tag_col = |tag: &str| -> egui::Color32 {
        match tag {
            "FIXME" => t.err,
            "TODO" => t.warn,
            _ => t.accent,
        }
    };

    egui::ScrollArea::vertical()
        .id_salt("rea_todos")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // group hits by file, sorted by path
            let mut by_file: std::collections::BTreeMap<PathBuf, Vec<usize>> =
                std::collections::BTreeMap::new();
            for (i, h) in app.todos.hits.iter().enumerate() {
                by_file.entry(h.path.clone()).or_default().push(i);
            }
            for (path, idxs) in by_file {
                let name = path
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.display().to_string());
                ui.add_space(2.0);
                egui::CollapsingHeader::new(
                    egui::RichText::new(format!("📄 {} ({})", name, idxs.len())).size(11.5),
                )
                .id_salt(format!("todo_{}", path.display()))
                .default_open(true)
                .show(ui, |ui| {
                    for i in idxs {
                        let hit = &app.todos.hits[i];
                        let row = egui::RichText::new(format!("  {} {}", hit.line + 1, hit.text))
                            .size(11.0)
                            .color(tag_col(&hit.tag).gamma_multiply(0.85));
                        if ui
                            .add(egui::Button::new(row).frame(false))
                            .on_hover_text(path.display().to_string())
                            .clicked()
                        {
                            let p = hit.path.clone();
                            let ln = hit.line;
                            app.open_path(&p);
                            app.pending_scroll = Some(ln);
                        }
                    }
                });
            }
            ui.add_space(6.0);
        });
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ext_whitelist() {
        assert!(ext_ok("main.rs"));
        assert!(ext_ok("APP.TS"));
        assert!(ext_ok("Makefile"));
        assert!(!ext_ok("photo.png"));
        assert!(!ext_ok("archive.zip"));
    }

    #[test]
    fn regex_finds_tags_not_words() {
        let re = regex();
        assert!(re.is_match("// TODO: fix this"));
        assert!(re.is_match("FIXME urgent"));
        assert!(re.is_match("# XXX hack"));
        assert!(!re.is_match("todolist"));
        assert!(!re.is_match("autodidact"));
        let m = re.find("  // TODO: write tests").unwrap();
        assert_eq!(m.as_str(), "TODO");
    }

    #[test]
    fn scan_finds_and_skips() {
        let base = std::env::temp_dir().join(format!("rea_todo_test_{}", std::process::id()));
        let sub = base.join("src");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::create_dir_all(base.join(".git")).unwrap();
        std::fs::write(sub.join("a.rs"), "fn main() {}\n// TODO: implement\n").unwrap();
        std::fs::write(base.join("b.txt"), "line1\n/* FIXME crash */\n").unwrap();
        std::fs::write(base.join(".git").join("c.rs"), "// TODO inside git\n").unwrap();
        std::fs::write(base.join("d.png"), "not scanned").unwrap();

        let mut hits = Vec::new();
        let mut files = 0;
        scan_dir(&base, &mut hits, &mut files, 0);
        assert_eq!(files, 2); // a.rs + b.txt (.git skipped, d.png not whitelisted)
        assert_eq!(hits.len(), 2);
        // sort order: ".git" < "b.txt" < "d.png" < "src" → b.txt (FIXME) first
        assert_eq!(hits[0].tag, "FIXME");
        assert_eq!(hits[0].line, 1);
        assert_eq!(hits[1].tag, "TODO");
        assert_eq!(hits[1].line, 1);

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn binary_file_skipped() {
        let base = std::env::temp_dir().join(format!("rea_todo_bin_{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let mut bytes = vec![0u8, 1, 2, 3];
        bytes.extend_from_slice(b"TODO in binary");
        std::fs::write(base.join("x.rs"), &bytes).unwrap();
        let mut hits = Vec::new();
        let mut files = 0;
        scan_dir(&base, &mut hits, &mut files, 0);
        assert_eq!(files, 1); // counted as visited but produces no hits
        assert!(hits.is_empty());
        std::fs::remove_dir_all(&base).ok();
    }
}
