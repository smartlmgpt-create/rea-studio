//! Workspace-wide full-text search & replace (Ctrl+Shift+F).
//! Runs on a worker thread; results stream into a tree view in the sidebar.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui;
use egui::{Align, Color32, Rounding, Sense, Stroke, vec2};

use crate::app::App;
use crate::theme::Theme;

const MAX_HITS_PER_FILE: usize = 200;
const MAX_FILE_BYTES: usize = 1024 * 1024;
const SEARCH_THREADS: usize = 8;

#[derive(Clone)]
pub struct WsHit {
    pub line_no: usize, // 0-based
    pub range: (usize, usize), // char range within the LINE preview
    pub preview: String,
}

#[derive(Clone)]
pub struct WsFileHits {
    pub path: PathBuf,
    pub hits: Vec<WsHit>,
}

pub struct WsSearch {
    pub query: String,
    pub replace: String,
    pub show_replace: bool,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub use_regex: bool,
    pub results: Arc<Mutex<Vec<WsFileHits>>>,
    pub busy: Arc<AtomicBool>,
    pub replace_busy: Arc<AtomicBool>,
    pub progress: Arc<Mutex<String>>,
    pub sig: Option<(String, bool, bool, bool)>,
    pub expanded: HashSet<PathBuf>,
    pub total_files: Arc<Mutex<usize>>,
}

impl Default for WsSearch {
    fn default() -> Self {
        Self {
            query: String::new(),
            replace: String::new(),
            show_replace: false,
            case_sensitive: false,
            whole_word: false,
            use_regex: false,
            results: Arc::new(Mutex::new(Vec::new())),
            busy: Arc::new(AtomicBool::new(false)),
            replace_busy: Arc::new(AtomicBool::new(false)),
            progress: Arc::new(Mutex::new(String::new())),
            sig: None,
            expanded: HashSet::new(),
            total_files: Arc::new(Mutex::new(0)),
        }
    }
}

impl WsSearch {
    pub fn hit_count(&self) -> usize {
        self.results.lock().unwrap().iter().map(|f| f.hits.len()).sum()
    }

    pub fn file_count(&self) -> usize {
        self.results.lock().unwrap().len()
    }
}

// ---------------------------------------------------------------- worker

fn compile(q: &str, cs: bool, ww: bool, rx: bool) -> Result<Matcher, String> {
    if rx {
        let mut pat = q.to_string();
        if ww {
            pat = format!(r"\b(?:{})\b", pat);
        }
        let re = regex::RegexBuilder::new(&pat)
            .case_insensitive(!cs)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Matcher::Regex(re))
    } else {
        if ww {
            // build a regex with escaped literal
            let esc = regex::escape(q);
            let pat = format!(r"\b(?:{})\b", esc);
            let re = regex::RegexBuilder::new(&pat)
                .case_insensitive(!cs)
                .build()
                .map_err(|e| e.to_string())?;
            Ok(Matcher::Regex(re))
        } else if !cs {
            // case-insensitive literal: to_lowercase() can CHANGE byte lengths
            // (e.g. 'İ' folds to 3 bytes), shifting every offset and panicking
            // at the byte-slice call sites. Route it through the regex engine
            // instead — its matches are always valid byte ranges of the
            // original line.
            let re = regex::RegexBuilder::new(&regex::escape(q))
                .case_insensitive(true)
                .size_limit(10_000_000)
                .build()
                .map_err(|e| e.to_string())?;
            Ok(Matcher::Regex(re))
        } else {
            Ok(Matcher::Literal {
                needle: q.to_string(),
                cs,
            })
        }
    }
}

enum Matcher {
    Literal { needle: String, cs: bool },
    Regex(regex::Regex),
}

impl Matcher {
    fn find_in_line(&self, line: &str) -> Vec<(usize, usize)> {
        match self {
            Matcher::Literal { needle, .. } => {
                // byte scan on the ORIGINAL line — returned offsets are valid
                // byte ranges of `line` itself
                let mut out = Vec::new();
                let n = needle.len();
                if n == 0 || line.len() < n {
                    return out;
                }
                let hb = line.as_bytes();
                let nb = needle.as_bytes();
                let mut off = 0usize;
                while off + n <= hb.len() {
                    if &hb[off..off + n] == nb {
                        out.push((off, off + n));
                        off += n;
                    } else {
                        off += 1;
                    }
                }
                out
            }
            Matcher::Regex(re) => re.find_iter(line).map(|m| (m.start(), m.end())).collect(),
        }
    }
}

pub fn start_search(app: &mut App) {
    let q = app.ws.query.trim().to_string();
    if q.is_empty() {
        return;
    }
    let Some(ws) = app.workspace.clone() else {
        app.set_status("Open a folder first to search");
        return;
    };
    let matcher = match compile(&q, app.ws.case_sensitive, app.ws.whole_word, app.ws.use_regex) {
        Ok(m) => Arc::new(m),
        Err(e) => {
            app.set_status(format!("Bad pattern: {e}"));
            return;
        }
    };
    // walk the workspace FRESH (respects the raised caps + dotfile setting)
    app.refresh_tree();
    let files = app.ws_files.clone();
    *app.ws.total_files.lock().unwrap() = files.len();
    app.ws.results.lock().unwrap().clear();
    app.ws.busy.store(true, Ordering::SeqCst);

    let results = Arc::clone(&app.ws.results);
    let busy = Arc::clone(&app.ws.busy);
    let progress = Arc::clone(&app.ws.progress);
    let q2 = q.clone();

    std::thread::spawn(move || {
        let total = files.len().max(1);
        // parallel scan: N workers pull file paths from a shared queue
        let queue: Arc<Mutex<Vec<PathBuf>>> = Arc::new(Mutex::new(files));
        let done = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let n_workers = SEARCH_THREADS
            .min(std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4))
            .max(1);
        let mut handles = Vec::new();
        for _ in 0..n_workers {
            let queue = Arc::clone(&queue);
            let matcher = Arc::clone(&matcher);
            let done = Arc::clone(&done);
            let progress = Arc::clone(&progress);
            handles.push(std::thread::spawn(move || {
                let mut local: Vec<WsFileHits> = Vec::new();
                loop {
                    let path = {
                        let mut q = queue.lock().unwrap();
                        q.pop()
                    };
                    let Some(path) = path else { break };
                    let Ok(meta) = std::fs::metadata(&path) else { continue };
                    if meta.len() > MAX_FILE_BYTES as u64 {
                        continue;
                    }
                    // decode like the editor does (UTF-16/BOM/CRLF-aware)
                    if let Ok(bytes) = std::fs::read(&path) {
                        let (text, _, _) = crate::util::decode_text(&bytes);
                        if text.contains('\0') {
                            continue; // binary
                        }
                        let mut hits = Vec::new();
                        for (li, line) in text.split('\n').enumerate() {
                            if hits.len() >= MAX_HITS_PER_FILE {
                                break;
                            }
                            let one = line.trim_end_matches('\r');
                            for (s, e) in matcher.find_in_line(one) {
                                let cs = one[..s].chars().count();
                                let ce = cs + one[s..e].chars().count();
                                hits.push(WsHit {
                                    line_no: li,
                                    range: (cs, ce),
                                    preview: one.chars().take(300).collect(),
                                });
                                if hits.len() >= MAX_HITS_PER_FILE {
                                    break;
                                }
                            }
                        }
                        if !hits.is_empty() {
                            local.push(WsFileHits { path, hits });
                        }
                    }
                    let d = done.fetch_add(1, Ordering::Relaxed) + 1;
                    if d % 25 == 0 {
                        *progress.lock().unwrap() = format!("Searching… {}/{}", d, total);
                    }
                }
                local
            }));
        }
        let mut out: Vec<WsFileHits> = Vec::new();
        for h in handles {
            if let Ok(mut part) = h.join() {
                out.append(&mut part);
            }
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        *progress.lock().unwrap() = format!("“{}” — done", q2);
        *results.lock().unwrap() = out;
        busy.store(false, Ordering::SeqCst);
        let _ = ws;
    });
}

/// Replace across the whole workspace (files with current hits).
/// Open documents are replaced SYNCHRONOUSLY in the editor (marked dirty,
/// undo recorded — autosave later persists them), so the old race between the
/// worker thread's fs::write and autosave writing the same file is gone.
pub fn replace_all(app: &mut App) {
    let files: Vec<PathBuf> = {
        let r = app.ws.results.lock().unwrap();
        if r.is_empty() {
            return;
        }
        r.iter().map(|f| f.path.clone()).collect()
    };
    let matcher = match compile(&app.ws.query, app.ws.case_sensitive, app.ws.whole_word, app.ws.use_regex) {
        Ok(m) => Arc::new(m),
        Err(_) => return,
    };
    let rep = app.ws.replace.clone();
    let busy = Arc::clone(&app.ws.replace_busy);
    let progress = Arc::clone(&app.ws.progress);

    // 1) open documents — main thread, keeps editor state consistent
    let mut disk_files: Vec<PathBuf> = Vec::new();
    for path in &files {
        let doc_idx = app.docs.iter().position(|d| d.path.as_ref() == Some(path));
        match doc_idx {
            Some(i) => {
                let text = app.docs[i].text.clone();
                let new_text = apply_replace(&matcher, &text, &rep);
                if new_text != text {
                    app.docs[i].text = new_text;
                    app.docs[i].dirty = true;
                    app.docs[i].last_edit = Some(std::time::Instant::now());
                    let txt_c = app.docs[i].text.clone();
                    app.docs[i].undo.force(&txt_c, None);
                    app.lsp_notify_change(i);
                }
            }
            None => disk_files.push(path.clone()),
        }
    }
    app.word_cache_sig = (usize::MAX, 0);
    app.find.sig = None;

    // 2) files not open in the editor — background thread as before
    busy.store(true, Ordering::SeqCst);
    std::thread::spawn(move || {
        let total = disk_files.len().max(1);
        for (i, path) in disk_files.iter().enumerate() {
            *progress.lock().unwrap() = format!("Replacing… {}/{}", i + 1, total);
            let Ok(bytes) = std::fs::read(path) else { continue };
            let (text, bom, crlf) = crate::util::decode_text(&bytes);
            if text.contains('\0') {
                continue;
            }
            let new_text = apply_replace(&matcher, &text, &rep);
            if new_text != text {
                let mut out = String::new();
                if bom {
                    out.push('\u{feff}');
                }
                if crlf {
                    out.push_str(&new_text.replace('\n', "\r\n"));
                } else {
                    out.push_str(&new_text);
                }
                let _ = std::fs::write(path, out);
            }
        }
        *progress.lock().unwrap() = "Replace finished".to_string();
        busy.store(false, Ordering::SeqCst);
    });
}

fn apply_replace(matcher: &Matcher, text: &str, rep: &str) -> String {
    match matcher {
        Matcher::Regex(re) => re.replace_all(text, rep).to_string(),
        Matcher::Literal { needle, cs } => {
            if *cs {
                text.replace(needle.as_str(), rep)
            } else {
                // case-insensitive literal replace (byte scan, safe for UTF-8)
                let hay = text.to_lowercase();
                let ned = needle.to_lowercase();
                let nb = hay.as_bytes();
                let n = ned.len();
                let mut pieces: Vec<(usize, usize)> = Vec::new(); // byte ranges in original
                let mut off = 0usize;
                while off + n <= nb.len() {
                    if &nb[off..off + n] == ned.as_bytes() {
                        // validate char boundary
                        if text.is_char_boundary(off) && text.is_char_boundary(off + n) {
                            pieces.push((off, off + n));
                            off += n;
                        } else {
                            off += 1;
                        }
                    } else {
                        off += 1;
                    }
                }
                let mut out = String::with_capacity(text.len());
                let mut last = 0usize;
                for (s, e) in pieces {
                    out.push_str(&text[last..s]);
                    out.push_str(rep);
                    last = e;
                }
                out.push_str(&text[last..]);
                out
            }
        }
    }
}

// ---------------------------------------------------------------- UI

pub fn show(app: &mut App, ui: &mut egui::Ui, t: &Theme) {
    let busy = app.ws.busy.load(Ordering::SeqCst);
    let rbusy = app.ws.replace_busy.load(Ordering::SeqCst);
    if busy || rbusy {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(150));
    }

    // ---- query row ----
    ui.horizontal(|ui| {
        let mut q = app.ws.query.clone();
        let resp = ui.add(
            egui::TextEdit::singleline(&mut q)
                .hint_text("Search in workspace…")
                .desired_width(ui.available_width() - 28.0),
        );
        if resp.changed() {
            app.ws.query = q;
        }
        if ui.button("⌕").clicked() {
            crate::wssearch::start_search(app);
        }
        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            crate::wssearch::start_search(app);
        }
    });
    ui.horizontal(|ui| {
        ui.toggle_value(&mut app.ws.case_sensitive, "Aa").on_hover_text("Match case");
        ui.toggle_value(&mut app.ws.whole_word, "|ab|").on_hover_text("Whole word");
        ui.toggle_value(&mut app.ws.use_regex, ".*").on_hover_text("Regular expression");
        if ui.toggle_value(&mut app.ws.show_replace, "⇄").on_hover_text("Replace").changed() {
            // no-op, layout only
        }
    });
    ui.add_space(2.0);

    if app.ws.show_replace {
        ui.horizontal(|ui| {
            let mut rp = app.ws.replace.clone();
            let resp = ui.add(
                egui::TextEdit::singleline(&mut rp)
                    .hint_text("Replace with…")
                    .desired_width(ui.available_width() - 96.0),
            );
            if resp.changed() {
                app.ws.replace = rp;
            }
            if ui.add_enabled(!rbusy && app.ws.file_count() > 0, egui::Button::new("Replace All")).clicked() {
                crate::wssearch::replace_all(app);
            }
        });
    }

    ui.separator();

    // ---- results ----
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if !app.workspace.is_some() {
                ui.label(
                    egui::RichText::new("Open a folder to search inside it.")
                        .size(12.0)
                        .color(t.text_faint),
                );
                return;
            }
            let results = app.ws.results.lock().unwrap().clone();
            let progress = app.ws.progress.lock().unwrap().clone();
            if !progress.is_empty() {
                ui.label(egui::RichText::new(&progress).size(11.0).color(t.text_faint));
            }
            if results.is_empty() {
                if !busy && !app.ws.query.is_empty() && !progress.starts_with("Searching") {
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new("No results")
                            .size(12.0)
                            .color(t.text_faint),
                    );
                }
                return;
            }
            let total_hits: usize = results.iter().map(|f| f.hits.len()).sum();
            ui.label(
                egui::RichText::new(format!(
                    "{} matches in {} files",
                    total_hits,
                    results.len()
                ))
                .size(11.0)
                .color(t.text_dim),
            );
            ui.add_space(2.0);

            for f in &results {
                let open = app.ws.expanded.contains(&f.path);
                let name = f
                    .path
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                let (chev_col, _) = (t.text_dim, 0.0);
                let fr = egui::Frame::default()
                    .fill(Color32::TRANSPARENT)
                    .rounding(Rounding::same(4.0))
                    .inner_margin(egui::Margin::symmetric(4.0, 2.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            crate::icons::chevron(&ui.painter(), ui.cursor().left_center() + vec2(6.0, 0.0), open, chev_col);
                            ui.add_space(14.0);
                            let ext = f.path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
                            crate::icons::badge(ui, &ext);
                            ui.label(egui::RichText::new(&name).size(12.5).strong().color(t.text));
                            ui.label(
                                egui::RichText::new(format!("{}", f.hits.len()))
                                    .size(10.5)
                                    .color(t.text_faint),
                            );
                        });
                    });
                if fr.response.interact(Sense::click()).clicked() {
                    if open {
                        app.ws.expanded.remove(&f.path);
                    } else {
                        app.ws.expanded.insert(f.path.clone());
                    }
                }
                if open {
                    for h in &f.hits {
                        let row = egui::Frame::default()
                            .fill(Color32::TRANSPARENT)
                            .rounding(Rounding::same(3.0))
                            .inner_margin(egui::Margin::symmetric(4.0, 1.0))
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width() - 8.0);
                                ui.horizontal(|ui| {
                                    ui.add_space(22.0);
                                    ui.monospace(
                                        egui::RichText::new(format!("{}", h.line_no + 1))
                                            .size(10.5)
                                            .color(t.text_faint),
                                    );
                                    // preview with highlight
                                    let chars: Vec<char> = h.preview.chars().collect();
                                    let (s, e) = h.range;
                                    let pre: String = chars[..s.min(chars.len())].iter().collect();
                                    let mid: String = chars[s.min(chars.len())..e.min(chars.len())].iter().collect();
                                    let post: String = chars[e.min(chars.len())..].iter().collect();
                                    ui.monospace(egui::RichText::new(pre).size(11.5).color(t.text_dim));
                                    ui.monospace(egui::RichText::new(mid).size(11.5).strong().color(t.accent));
                                    ui.monospace(egui::RichText::new(post).size(11.5).color(t.text_dim));
                                });
                            });
                        let rr = row.response.interact(Sense::click());
                        if rr.clicked() {
                            app.open_path(&f.path);
                            // scroll to the hit line
                            let text = app.docs.iter().find(|d| d.path.as_ref() == Some(&f.path)).map(|d| d.text.clone());
                            if let Some(text) = text {
                                let cc = crate::util::line_col_to_char(&text, h.line_no, 0);
                                app.pending_scroll = Some(h.line_no);
                                let _ = cc;
                                // pre-fill find so the match is visible & navigable
                                app.find.query = app.ws.query.clone();
                                app.find.sig = None;
                                app.find.idx = 0;
                            }
                        }
                        rr.on_hover_cursor(egui::CursorIcon::PointingHand);
                    }
                }
            }
            let _ = Stroke::new(1.0, t.border);
            let _ = Align::LEFT;
        });
}
