use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use eframe::egui;
use egui::Color32;
use serde_json::json;

use crate::highlight::Highlighter;
use crate::icons;
use crate::overlay::{PalItem, Palette, PaletteMode};
use crate::theme::{self, Theme};
use crate::undo::UndoStack;

use crate::{dap, gitpanel, lsp, wssearch};
use crate::github;

// v1.6 plugins: live server + rest client states live on App
use crate::{liveserver, restclient};

// ---------------------------------------------------------------- documents

/// One open editor document (tab).
pub struct Doc {
    pub path: Option<PathBuf>,
    pub name: String,
    pub text: String,
    pub dirty: bool,
    /// real undo/redo history (survives reopen via backups dir)
    pub undo: UndoStack,
    /// last known disk mtime + len (file watcher)
    pub disk_mtime: Option<SystemTime>,
    pub disk_len: u64,
    /// Some(disk_text) when the file changed on disk while we have unsaved edits
    pub conflict: Option<String>,
    /// unsaved content recovered from the backup dir on open
    pub recovered: Option<String>,
    /// time of last edit (autosave + lsp change debounce)
    pub last_edit: Option<Instant>,
    /// LSP document version counter
    pub lsp_version: i32,
    /// file had a BOM — re-added on save
    pub bom: bool,
    /// file used CRLF line endings — restored on save
    pub crlf: bool,
    /// v1.9: last known line count (bookmark line-shift bookkeeping)
    pub last_lc: usize,
}

impl Doc {
    pub fn new(path: Option<PathBuf>, name: String, text: String) -> Self {
        let (mtime, len) = path
            .as_ref()
            .and_then(|p| std::fs::metadata(p).ok())
            .map(|m| (m.modified().ok(), m.len()))
            .unwrap_or((None, 0));
        // FIX (#1): the stack MUST start with the initial text as its base
        // state. With `UndoStack::default()` the first edit became entry 0
        // and Ctrl+Z had nothing to undo — the whole undo history was dead
        // for every freshly opened file without persisted backups.
        let undo = UndoStack::with_initial(&text);
        let last_lc = text.matches('\n').count();
        Self {
            path,
            name,
            text,
            dirty: false,
            undo,
            disk_mtime: mtime,
            disk_len: len,
            conflict: None,
            recovered: None,
            last_edit: None,
            lsp_version: 1,
            bom: false,
            crlf: false,
            last_lc,
        }
    }

    /// backup key for undo persistence (stable per path or per name)
    pub fn backup_key(&self) -> String {
        match &self.path {
            Some(p) => p.display().to_string(),
            None => format!("untitled:{}", self.name),
        }
    }

    /// The exact bytes that belong on disk: BOM + CRLF restored.
    pub fn save_text(&self) -> String {
        let mut out = String::with_capacity(self.text.len() + 4);
        if self.bom {
            out.push('\u{feff}');
        }
        if self.crlf {
            out.push_str(&self.text.replace('\n', "\r\n"));
        } else {
            out.push_str(&self.text);
        }
        out
    }
}

// ------------------------------------------------ multi-occurrence editing

/// Ctrl+D / Ctrl+Shift+L state: every occurrence region (char range) that a
/// subsequent typed keystroke replaces simultaneously.
#[derive(Clone)]
pub struct MultiEdit {
    pub doc: usize,
    /// insertion/replacement regions (char indices), in text order
    pub regions: Vec<(usize, usize)>,
    /// region the visible caret follows
    pub caret_region: usize,
}

// ---------------------------------------------------------------- theme key

#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ThemeKey {
    Dark,
    Light,
    Custom(PathBuf),
}

#[derive(Clone, Copy, PartialEq)]
pub enum PanelMode {
    Explorer,
    Search,
    Extensions,
    Settings,
    /// v1.9: function list (Notepad++ Function List counterpart)
    Outline,
    /// v2.1: workspace TODO scanner
    Todos,
}

/// v1.6: tabs of the redesigned bottom panel (v1.7: + GIT).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BottomTab {
    Terminal,
    Rest,
    Git,
}

#[derive(Clone)]
pub struct UserTheme {
    pub path: PathBuf,
    pub theme: Theme,
}

// ---------------------------------------------------------------- settings (persisted)

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Settings {
    #[serde(default)]
    theme: Option<ThemeKey>,
    #[serde(default)]
    font: Option<String>,
    #[serde(default)]
    recents: Vec<String>,
    #[serde(default)]
    terminal_visible: bool,
    #[serde(default)]
    recent_folders: Vec<String>,
    /// v1.9: reopen the last session's tabs on start (None = on)
    #[serde(default)]
    session_restore: Option<bool>,
    /// v1.9: update-check manifest URL (None = built-in default)
    #[serde(default)]
    update_url: Option<String>,
    /// v1.9: file path -> sorted bookmark lines (persisted like breakpoints)
    #[serde(default)]
    bookmarks: std::collections::BTreeMap<String, Vec<usize>>,
    #[serde(default)]
    editor: EditorSettings,
}

/// v1.9: update-check state (silent thread probe of a version manifest).
#[derive(Clone, PartialEq)]
pub enum UpdState {
    Idle,
    Checking,
    UpToDate,
    Available { ver: String, url: String, notes: String },
    Failed,
}

/// User-tunable editor preferences (Settings panel).
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct EditorSettings {
    pub font_size: f32,
    pub tab_size: usize,
    pub word_wrap: bool,
    pub autosave: bool,
    pub breadcrumbs: bool,
    pub terminal_on_boot: bool,
    pub auto_pair: bool,
    pub auto_indent: bool,
    /// show dotfiles (.gitignore etc.) in the Explorer + workspace files (#14)
    #[serde(default)]
    pub show_dotfiles: bool,
    /// ext -> lsp command, e.g. "rs" -> "rust-analyzer"
    pub lsp: std::collections::BTreeMap<String, String>,
    /// v1.6: plugin id -> enabled (missing = enabled by default)
    #[serde(default)]
    pub plugin_enabled: std::collections::BTreeMap<String, bool>,
    /// v1.6: Live Server port
    #[serde(default)]
    pub live_port: u16,
    /// v1.7: use the real ConPTY terminal (fallback = legacy pipe mode)
    #[serde(default = "d_true")]
    pub terminal_conpty: bool,
    /// v1.7: run the auto-formatter on every save
    #[serde(default)]
    pub format_on_save: bool,
    /// v1.7: Live Server serves HTTPS (self-signed) on live_https_port
    #[serde(default)]
    pub live_https: bool,
    /// v1.7: HTTPS port for the Live Server
    #[serde(default = "d_https_port")]
    pub live_https_port: u16,
    /// v1.7: show the editor minimap
    #[serde(default = "d_true")]
    pub minimap: bool,
    /// v1.7: exts already probed for an auto-detected LSP server
    #[serde(default)]
    pub lsp_probed: std::collections::BTreeSet<String>,
}

fn d_true() -> bool {
    true
}
fn d_https_port() -> u16 {
    5540
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            font_size: 14.0,
            tab_size: 4,
            word_wrap: false,
            autosave: false,
            breadcrumbs: true,
            terminal_on_boot: false,
            auto_pair: true,
            auto_indent: true,
            show_dotfiles: false,
            lsp: std::collections::BTreeMap::new(),
            plugin_enabled: std::collections::BTreeMap::new(),
            live_port: 5500,
            terminal_conpty: true,
            format_on_save: false,
            live_https: false,
            live_https_port: 5540,
            minimap: true,
            lsp_probed: std::collections::BTreeSet::new(),
        }
    }
}

// ---------------------------------------------------------------- find state

pub struct FindState {
    pub open: bool,
    pub show_replace: bool,
    pub query: String,
    pub replace: String,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub use_regex: bool,
    pub matches: Vec<(usize, usize)>, // flat char ranges
    pub idx: usize,
    pub focus_req: bool,
    pub replace_focus_req: bool,
    pub input_focused: bool,
    /// sig uses the doc's lsp_version (edit epoch) — the old byte-length sig
    /// missed edits that didn't change the length (stale matches)
    pub sig: Option<(String, usize, usize, u64, bool, bool, bool)>,
    pub error: Option<String>,
}

impl Default for FindState {
    fn default() -> Self {
        Self {
            open: false,
            show_replace: false,
            query: String::new(),
            replace: String::new(),
            case_sensitive: false,
            whole_word: false,
            use_regex: false,
            matches: Vec::new(),
            idx: 0,
            focus_req: false,
            replace_focus_req: false,
            input_focused: false,
            sig: None,
            error: None,
        }
    }
}

/// Modal text-input dialog state (conditional breakpoints + FS operations).
#[derive(Clone)]
pub struct InputDialog {
    pub title: String,
    pub label: String,
    pub value: String,
    /// "bp:<path>|<line>" | "newfile:<dir>" | "newfolder:<dir>" | "rename:<path>"
    pub action: String,
}

// ---------------------------------------------------------------- app

pub struct App {
    pub ctx: egui::Context,
    pub hl: Highlighter,

    // documents
    pub docs: Vec<Doc>,
    pub active: Option<usize>,
    pub cursor: Option<(usize, usize)>,
    pub untitled_counter: usize,

    // workspace
    pub workspace: Option<PathBuf>,
    pub dir_cache: HashMap<PathBuf, Vec<crate::bars::Node>>,
    pub collapsed: HashSet<PathBuf>,
    pub ws_files: Vec<PathBuf>,

    // ui state
    pub show_sidebar: bool,
    pub panel_mode: PanelMode,
    pub search_query: String,
    pub search_tab_files: bool,
    pub about_open: bool,
    pub editor_rect: egui::Rect,
    /// logical LINE number of the cursor (row≠line when word-wrap is on)
    pub cursor_line_no: Option<usize>,

    // theme + animation
    pub theme_key: ThemeKey,
    pub theme_now: Theme,
    pub anim: Option<(Theme, Theme, Instant)>,

    // fonts
    pub custom_font: Option<String>,

    // palette
    pub palette: Option<Palette>,

    // find
    pub find: FindState,
    pub pending_scroll: Option<usize>,

    // terminal — v1.7: MULTIPLE sessions (ConPTY or legacy pipe backend)
    pub terms: Vec<crate::terminal::TermSession>,
    pub term_active: usize,
    pub term_id_counter: usize,
    /// focus action applied after the session is re-inserted into the vec
    pub term_pending_focus: Option<bool>,
    /// scroll action applied after the session is re-inserted into the vec
    pub term_pending_scroll: Option<i32>,
    pub terminal_visible: bool,
    pub term_focus_req: bool,
    pub bottom_tab: BottomTab,

    // ---- v1.6: plugins ----
    pub live: liveserver::LiveServer,
    pub rest: restclient::RestState,

    // ---- v1.7: git panel / split view / minimap / tab drag / formatter ----
    pub git_state: gitpanel::GitState,
    /// split editor: right pane shows this doc while the left stays active
    pub split_on: bool,
    pub split_doc: Option<usize>,
    pub split_cursor: Option<(usize, usize)>,
    pub split_sel: Option<(usize, usize)>,
    /// tab rects captured during the tab bar pass (for drag & drop reorder)
    pub tab_rects: Vec<(usize, egui::Rect)>,
    /// tab being dragged
    pub tab_drag: Option<usize>,
    /// formatter availability cache (tool -> installed?)
    pub fmt_cache: HashMap<String, bool>,

    // code suggestions (autocomplete)
    pub suggest: Option<crate::complete::Suggest>,
    pub editor_focused: bool,
    pub cursor_char: Option<usize>,
    pub suggest_pos: Option<egui::Pos2>,
    pub word_cache: Vec<String>,
    pub word_cache_sig: (usize, usize),
    pub suppress_len: Option<usize>,

    // github clone
    pub github: Option<github::GitHubDialog>,
    pub clone_job: Option<github::CloneJob>,

    // recents + status
    pub recents: Vec<PathBuf>,
    pub status: (String, Option<Instant>),
    pub last_title: String,

    // extensions data
    pub user_themes: Vec<UserTheme>,
    pub font_list: Vec<String>,

    // config dir
    pub(crate) cfg_dir: PathBuf,

    // ---- v1.4: editing engine ----
    pub settings: EditorSettings,
    /// multi-occurrence editing (Ctrl+D / Ctrl+Shift+L)
    pub multi: Option<MultiEdit>,
    /// current TextEdit selection (char range) captured from TextEditState
    pub sel_range: Option<(usize, usize)>,
    /// last disk-watch sweep
    pub last_disk_check: Instant,
    /// font size applied to egui styles last (avoid redundant set)
    pub applied_font_size: f32,

    // ---- v1.4: workspace search ----
    pub ws: wssearch::WsSearch,

    // ---- v1.4: recent folders ----
    pub recent_folders: Vec<PathBuf>,

    // ---- v1.4: LSP ----
    pub lsp: lsp::LspHub,
    /// diagnostics per open-doc path
    pub diags: HashMap<PathBuf, Vec<lsp::LspDiag>>,
    pub lsp_version_counter: i32,

    // ---- v1.4: debugger ----
    pub debug: Option<dap::DapSession>,
    pub breakpoints: HashMap<PathBuf, Vec<dap::Bp>>,
    pub launch_cfg: Option<dap::LaunchCfg>,
    /// folders whose launch.json the user approved in this session (v1.7.3)
    pub dap_trusted: std::collections::HashSet<String>,
    /// pending "trust this workspace?" request shown before any launch.json runs
    pub dap_trust_req: Option<dap::TrustReq>,

    // ---- v1.5 additions ----
    /// tab close awaiting "Save changes?" confirmation
    pub pending_close: Option<usize>,
    /// exit awaiting "Save changes?" confirmation
    pub exit_confirm: bool,
    /// window close allowed (set once the user chose Save/Discard)
    pub exit_allow: bool,
    /// conditional breakpoint / new-file / rename modal
    pub input_dialog: Option<InputDialog>,
    /// track dir mtimes so files created/deleted by the TERMINAL invalidate
    /// the explorer cache (#14)
    pub dir_mtimes: HashMap<PathBuf, SystemTime>,
    pub last_dir_check: Instant,
    /// watch-panel input + "break on exceptions" mirror (dap UI)
    pub watch_input: String,
    pub exc_sent: bool,
    /// hover-eval state: word, screen pos, last request time
    pub hover_eval: Option<(String, egui::Pos2, Instant)>,
    /// completion word-cache throttle (#18)
    pub word_cache_time: Option<Instant>,

    // ---- v1.9: bookmarks / zoom / session / outline / update check ----
    /// file path -> sorted bookmark lines (Ctrl+F2 toggles, F2 navigates)
    pub bookmarks: std::collections::BTreeMap<PathBuf, Vec<usize>>,
    /// editor text zoom (1.0 = default) — Ctrl+wheel / Ctrl+= / Ctrl+- / Ctrl+0
    pub zoom: f32,
    /// reopen last session's tabs on start (persisted)
    pub session_restore: bool,
    /// update-check manifest URL override (persisted)
    pub update_url: Option<String>,
    /// pending session cursors consumed by restore_session()
    sess_cursors: Vec<(String, usize, usize)>,
    sess_active: usize,
    /// outline (function list) cache signature: (doc idx, epoch, ext)
    pub outline_sig: (usize, i32, String),
    pub outline_items: Vec<crate::outline::Sym>,
    /// update check state
    pub upd: UpdState,
    upd_rx: Option<std::sync::mpsc::Receiver<String>>,

    // ---- v2.0: macro + column editing ----
    /// keyboard macro being recorded (None = not recording)
    pub macro_rec: Option<Vec<crate::macros::MEv>>,
    /// column (block) selection state — Alt+Shift+Arrows
    pub colsel: Option<crate::colsel::ColSel>,

    // ---- v2.1: diff / todo / tools / stats ----
    /// side-by-side file compare (Tools → Compare Two Files…)
    pub diff: Option<crate::diffview::DiffView>,
    /// TODO scanner state (hits of the last scan)
    pub todos: crate::todopanel::TodoState,
    /// background scan job results
    pub(crate) todo_rx: Option<std::sync::mpsc::Receiver<(Vec<crate::todopanel::TodoHit>, usize)>>,
    /// Document Statistics window visibility (Tools → Document Statistics)
    pub stats_open: bool,
    /// cached stats snapshot shown in the window
    pub stats: Option<crate::texttools::DocStats>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let ctx = cc.egui_ctx.clone();

        let cfg_dir = config_dir();
        let _ = std::fs::create_dir_all(cfg_dir.join("themes"));
        let _ = std::fs::create_dir_all(cfg_dir.join("fonts"));

        // load settings
        let settings: Settings = std::fs::read_to_string(cfg_dir.join("settings.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        let recents = settings
            .recents
            .iter()
            .map(PathBuf::from)
            .collect::<Vec<_>>();

        let recent_folders = settings
            .recent_folders
            .iter()
            .map(PathBuf::from)
            .collect::<Vec<_>>();

        let editor_settings = settings.editor.clone();

        let mut app = Self {
            ctx: ctx.clone(),
            hl: Highlighter::new(),
            docs: Vec::new(),
            active: None,
            cursor: None,
            untitled_counter: 0,
            workspace: None,
            dir_cache: HashMap::new(),
            collapsed: HashSet::new(),
            ws_files: Vec::new(),
            show_sidebar: true,
            panel_mode: PanelMode::Explorer,
            search_query: String::new(),
            search_tab_files: true,
            about_open: false,
            editor_rect: egui::Rect::NOTHING,
            cursor_line_no: None,
            theme_key: settings.theme.clone().unwrap_or(ThemeKey::Dark),
            theme_now: theme::dark(),
            anim: None,
            custom_font: None,
            palette: None,
            find: FindState::default(),
            pending_scroll: None,
            terms: Vec::new(),
            term_active: 0,
            term_id_counter: 0,
            term_pending_focus: None,
            term_pending_scroll: None,
            terminal_visible: settings.terminal_visible || editor_settings.terminal_on_boot,
            term_focus_req: false,
            bottom_tab: BottomTab::Terminal,
            live: liveserver::LiveServer::default(),
            rest: restclient::RestState::default(),
            git_state: gitpanel::GitState::default(),
            split_on: false,
            split_doc: None,
            split_cursor: None,
            split_sel: None,
            tab_rects: Vec::new(),
            tab_drag: None,
            fmt_cache: HashMap::new(),
            suggest: None,
            editor_focused: false,
            cursor_char: None,
            suggest_pos: None,
            word_cache: Vec::new(),
            word_cache_sig: (usize::MAX, 0),
            suppress_len: None,
            github: None,
            clone_job: None,
            recents,
            status: (String::new(), None),
            last_title: String::new(),
            user_themes: Vec::new(),
            font_list: Vec::new(),
            cfg_dir,
            settings: editor_settings,
            multi: None,
            sel_range: None,
            last_disk_check: Instant::now(),
            applied_font_size: 0.0,
            ws: wssearch::WsSearch::default(),
            recent_folders,
            lsp: lsp::LspHub::default(),
            diags: HashMap::new(),
            lsp_version_counter: 1,
            debug: None,
            breakpoints: HashMap::new(),
            launch_cfg: None,
            dap_trusted: std::collections::HashSet::new(),
            dap_trust_req: None,
            pending_close: None,
            exit_confirm: false,
            exit_allow: false,
            input_dialog: None,
            dir_mtimes: HashMap::new(),
            last_dir_check: Instant::now(),
            watch_input: String::new(),
            exc_sent: false,
            hover_eval: None,
            word_cache_time: None,
            bookmarks: settings
                .bookmarks
                .iter()
                .map(|(k, v)| (PathBuf::from(k), v.clone()))
                .collect(),
            zoom: 1.0,
            session_restore: settings.session_restore != Some(false),
            update_url: settings.update_url.clone(),
            sess_cursors: Vec::new(),
            sess_active: 0,
            outline_sig: (usize::MAX, 0, String::new()),
            outline_items: Vec::new(),
            upd: UpdState::Idle,
            upd_rx: None,
            macro_rec: None,
            colsel: None,
            diff: None,
            todos: crate::todopanel::TodoState::default(),
            todo_rx: None,
            stats_open: false,
            stats: None,
        };

        // resolve initial theme (no animation on boot)
        let th = app.resolve_theme(&app.theme_key.clone());
        app.theme_now = th.clone();
        theme::apply(&ctx, &th);

        // apply saved font
        if let Some(f) = settings.font {
            if app.load_and_apply_font(&f) {
                app.custom_font = Some(f);
            }
        }

        // load user themes + fonts lists
        app.reload_user_themes();
        app.reload_font_list();

        // restore persisted breakpoints (#16)
        app.load_breakpoints();

        // v1.9: reopen last session + silent update check (background thread)
        if app.session_restore {
            app.restore_session();
        }
        app.upd_start(None);

        app
    }

    // ================================================================ helpers: config

    pub fn save_settings(&self) {
        let s = Settings {
            theme: Some(self.theme_key.clone()),
            font: self.custom_font.clone(),
            recents: self
                .recents
                .iter()
                .map(|p| p.display().to_string())
                .collect(),
            terminal_visible: self.terminal_visible,
            recent_folders: self
                .recent_folders
                .iter()
                .map(|p| p.display().to_string())
                .collect(),
            session_restore: Some(self.session_restore),
            update_url: self.update_url.clone(),
            bookmarks: self
                .bookmarks
                .iter()
                .map(|(k, v)| (k.display().to_string(), v.clone()))
                .collect(),
            editor: self.settings.clone(),
        };
        if let Ok(j) = serde_json::to_string_pretty(&s) {
            let _ = std::fs::write(self.cfg_dir.join("settings.json"), j);
        }
    }
    pub(crate) fn save_settings_pub(&self) {
        self.save_settings();
    }

    // ================================================================ v1.9: bookmarks

    pub fn toggle_bookmark_line(&mut self, i: usize, line: usize) {
        let Some(p) = self.docs.get(i).and_then(|d| d.path.clone()) else { return };
        let e = self.bookmarks.entry(p).or_default();
        match e.binary_search(&line) {
            Ok(pos) => {
                e.remove(pos);
            }
            Err(pos) => {
                e.insert(pos, line);
            }
        }
        self.save_settings();
    }

    /// Ctrl+F2 — bookmark the caret line of the active document
    pub fn toggle_bookmark(&mut self) {
        let Some(i) = self.active else { return };
        let Some((ln, _)) = self.cursor else { return };
        if self.docs[i].path.is_none() {
            self.set_status("Save the file first — bookmarks are per-file");
            return;
        }
        self.toggle_bookmark_line(i, ln);
    }

    /// F2 / Shift+F2 — jump to the next/previous bookmark in this file
    /// (wraps around, like Notepad++)
    pub fn bookmark_nav(&mut self, next: bool) {
        let Some(i) = self.active else { return };
        let Some(p) = self.docs[i].path.clone() else { return };
        let Some(list) = self.bookmarks.get(&p) else {
            self.set_status("No bookmarks in this file");
            return;
        };
        if list.is_empty() {
            self.set_status("No bookmarks in this file");
            return;
        }
        let cur = self.cursor.map(|(l, _)| l).unwrap_or(0);
        let line = if next {
            list.iter().find(|&&l| l > cur).copied().unwrap_or(list[0])
        } else {
            list.iter().rev().find(|&&l| l < cur).copied().unwrap_or(*list.last().unwrap())
        };
        self.goto_doc_line(i, line, 0);
        self.set_status(format!("Bookmark line {}", line + 1));
    }

    /// Alt+F2 — clear every bookmark of the current file
    pub fn bookmark_clear_doc(&mut self) {
        let Some(i) = self.active else { return };
        let Some(p) = self.docs[i].path.clone() else { return };
        if self.bookmarks.remove(&p).is_some() {
            self.save_settings();
            self.set_status("Bookmarks cleared");
        }
    }

    /// v1.9: keep bookmarks attached to their code when lines shift. Called
    /// after any edit on the active doc — lines at/after the caret line move
    /// by the line-count delta (insertions push them down, deletions pull up
    /// and clamp).
    pub fn bookmark_shift_after_edit(&mut self, i: usize) {
        let (lc, prev, path) = {
            let d = &self.docs[i];
            (d.text.matches('\n').count(), d.last_lc, d.path.clone())
        };
        self.docs[i].last_lc = lc;
        let Some(p) = path else { return };
        if lc == prev {
            return;
        }
        let delta = lc as isize - prev as isize;
        let cl = self.cursor.map(|(l, _)| l).unwrap_or(0);
        let Some(list) = self.bookmarks.get_mut(&p) else { return };
        let mut out: Vec<usize> = list
            .iter()
            .map(|&b| {
                if (b as isize) >= cl as isize {
                    let nb = b as isize + delta;
                    if nb < 0 { cl } else { nb as usize }
                } else {
                    b
                }
            })
            .collect();
        out.sort_unstable();
        out.dedup();
        *list = out;
    }

    /// v1.9: move the caret of doc `i` to (line, col) and scroll to it.
    pub fn goto_doc_line(&mut self, i: usize, line: usize, col: usize) {
        if i >= self.docs.len() {
            return;
        }
        let text = self.docs[i].text.clone();
        let cc = crate::util::line_col_to_char(&text, line, col);
        let id = egui::Id::new(format!("rea_editor_{i}"));
        let mut st =
            egui::widgets::text_edit::TextEditState::load(&self.ctx, id).unwrap_or_default();
        st.cursor
            .set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(cc))));
        st.store(&self.ctx, id);
        self.cursor = Some((line, col));
        self.cursor_line_no = Some(line);
        if self.active == Some(i) {
            self.pending_scroll = Some(line);
        }
        self.ctx.request_repaint();
    }

    // ================================================================ v1.9: zoom

    pub fn zoom_step(&mut self, factor: f32) {
        let z = (self.zoom * factor).clamp(0.5, 3.0);
        self.zoom = if (z - 1.0).abs() < 0.04 { 1.0 } else { z };
    }

    /// effective editor font size (settings.font_size × zoom), clamped
    pub fn editor_font_size(&self) -> f32 {
        (self.settings.font_size * self.zoom).clamp(9.0, 72.0)
    }

    // ================================================================ v2.0: macro

    /// Ctrl+Shift+R — start / stop macro recording.
    pub fn macro_record_toggle(&mut self) {
        if self.macro_rec.take().is_some() {
            self.set_status("Macro recording stopped — Ctrl+Shift+M to play");
        } else {
            self.macro_rec = Some(Vec::new());
            self.set_status("Macro recording… type now, Ctrl+Shift+R to stop");
        }
    }

    /// Ctrl+Shift+M — replay the recorded macro on the active document,
    /// landing as ONE undo step.
    pub fn macro_play(&mut self) {
        let evs = self.macro_rec.clone().unwrap_or_default();
        // playing while recording stops the recording first
        if self.macro_rec.take().is_some() {
            self.set_status("Recording stopped — playing macro");
        }
        let Some(i) = self.active else {
            self.set_status("No document open");
            return;
        };
        if evs.is_empty() {
            self.set_status("No macro recorded — Ctrl+Shift+R records one");
            return;
        }
        let text = self.docs[i].text.clone();
        let (cur, anchor) = match self.sel_range {
            Some((a, b)) => (b, a),
            None => (self.cursor_char.unwrap_or(0), self.cursor_char.unwrap_or(0)),
        };
        let mut eng = crate::macros::Engine::new(&text, cur, anchor);
        for ev in &evs {
            eng.event(ev);
        }
        let caret = eng.cur;
        self.docs[i].text = eng.text();
        let txt_c = self.docs[i].text.clone();
        self.docs[i].undo.force(&txt_c, Some(caret));
        self.docs[i].dirty = true;
        self.docs[i].last_edit = Some(Instant::now());
        self.word_cache_sig = (usize::MAX, 0);
        self.set_selection(i, (caret, caret));
        self.lsp_notify_change(i);
        self.bookmark_shift_after_edit(i);
        self.ctx.request_repaint();
        self.set_status(format!(
            "Macro played ({} steps) — one Ctrl+Z undoes it",
            evs.len()
        ));
    }

    // ================================================================ v2.0: column editing

    /// Start column mode at the current caret.
    pub fn col_start(&mut self) {
        let Some(i) = self.active else { return };
        let ch = self.cursor_char.unwrap_or(0);
        self.col_anchor_at(i, ch);
    }

    /// Alt+Shift+Click / palette — anchor column mode at a char index.
    pub fn col_anchor_at(&mut self, i: usize, at: usize) {
        if i >= self.docs.len() {
            return;
        }
        let text = self.docs[i].text.clone();
        let n = at.min(text.chars().count());
        let (l, c) = crate::util::char_to_line_col(&text, n);
        self.colsel = Some(crate::colsel::ColSel { doc: i, anchor: (l, c), cur: (l, c) });
        self.multi = None;
        self.set_selection(i, (n, n));
        self.set_status("Column mode — Alt+Shift+Arrows to extend, type to edit, Esc exits");
        self.ctx.request_repaint();
    }

    /// Extend the moving corner (delta lines / delta cols), clamped.
    pub fn col_extend(&mut self, dl: isize, dc: isize) {
        let Some(i) = self.active else { return };
        let n_lines = 1 + self.docs[i].text.matches('\n').count();
        if self.colsel.is_none() {
            self.col_start();
        }
        let Some(cs) = self.colsel.as_mut() else { return };
        cs.doc = i;
        cs.cur.0 = ((cs.cur.0 as isize + dl).max(0) as usize).min(n_lines - 1);
        cs.cur.1 = ((cs.cur.1 as isize + dc).max(0)) as usize;
        let (cl, cc) = cs.cur;
        let text = self.docs[i].text.clone();
        let ch = crate::util::line_col_to_char(&text, cl, cc);
        self.set_selection(i, (ch, ch));
        self.ctx.request_repaint();
    }

    /// Typed text replaces the selected block on every line (one undo step).
    pub fn col_type(&mut self, s: &str) {
        let Some(cs) = self.colsel else { return };
        let i = cs.doc;
        if i >= self.docs.len() {
            self.colsel = None;
            return;
        }
        let text = self.docs[i].text.clone();
        let (la, lb) = cs.lines();
        let (c0, c1) = cs.cols();
        let (t1, _) = crate::colsel::delete_at_col(&text, la, lb, c0, c1);
        let (new_text, new_col) = crate::colsel::insert_at_col(&t1, la, lb, c0, s);
        if let Some(cs) = self.colsel.as_mut() {
            cs.anchor.1 = new_col;
            cs.cur.1 = new_col;
        }
        self.col_commit(i, new_text, (la, new_col));
        if s.contains('\n') {
            self.bookmark_shift_after_edit(i);
        }
    }

    pub fn col_backspace(&mut self) {
        let Some(cs) = self.colsel else { return };
        let i = cs.doc;
        if i >= self.docs.len() {
            self.colsel = None;
            return;
        }
        let text = self.docs[i].text.clone();
        let (la, lb) = cs.lines();
        let (c0, c1) = cs.cols();
        let (cs_, ce_) = if c1 > c0 {
            (c0, c1) // delete the whole block
        } else if c0 > 0 {
            (c0 - 1, c0) // one char before the column
        } else {
            return; // at column 0 — nothing before it
        };
        let (new_text, new_col) = crate::colsel::delete_at_col(&text, la, lb, cs_, ce_);
        if let Some(cs) = self.colsel.as_mut() {
            cs.anchor.1 = new_col;
            cs.cur.1 = new_col;
        }
        self.col_commit(i, new_text, (la, new_col));
    }

    pub fn col_delete(&mut self) {
        let Some(cs) = self.colsel else { return };
        let i = cs.doc;
        if i >= self.docs.len() {
            self.colsel = None;
            return;
        }
        let text = self.docs[i].text.clone();
        let (la, lb) = cs.lines();
        let (c0, c1) = cs.cols();
        let (cs_, ce_) = if c1 > c0 {
            (c0, c1)
        } else {
            (c0, c0 + 1) // one char AT the column (no-op on short lines)
        };
        let (new_text, new_col) = crate::colsel::delete_at_col(&text, la, lb, cs_, ce_);
        if let Some(cs) = self.colsel.as_mut() {
            cs.anchor.1 = new_col;
            cs.cur.1 = new_col;
        }
        self.col_commit(i, new_text, (la, new_col));
    }

    /// Shared bookkeeping after a column edit: text in, one undo step,
    /// caret to the (line, col) corner, LSP + caches refreshed.
    fn col_commit(&mut self, i: usize, new_text: String, caret: (usize, usize)) {
        self.docs[i].text = new_text;
        let txt_c = self.docs[i].text.clone();
        let ch = crate::util::line_col_to_char(&txt_c, caret.0, caret.1);
        self.docs[i].undo.force(&txt_c, Some(ch));
        self.docs[i].dirty = true;
        self.docs[i].last_edit = Some(Instant::now());
        self.word_cache_sig = (usize::MAX, 0);
        self.set_selection(i, (ch, ch));
        self.lsp_notify_change(i);
        self.ctx.request_repaint();
    }

    /// Exit column mode (silent — used on click / navigation).
    pub fn col_exit(&mut self) {
        self.colsel = None;
    }

    // ================================================================ v1.9: session restore

    fn save_session(&self) {
        #[derive(serde::Serialize)]
        struct Sd {
            path: String,
            line: usize,
            col: usize,
        }
        #[derive(serde::Serialize)]
        struct Sess {
            active: usize,
            docs: Vec<Sd>,
        }
        let docs: Vec<Sd> = self
            .docs
            .iter()
            .enumerate()
            .filter_map(|(i, d)| {
                let p = d.path.as_ref()?;
                let (line, col) = if self.active == Some(i) {
                    self.cursor.unwrap_or((0, 0))
                } else {
                    (0, 0)
                };
                Some(Sd { path: p.display().to_string(), line, col })
            })
            .collect();
        let sess = Sess { active: self.active.unwrap_or(0), docs };
        if let Ok(j) = serde_json::to_string(&sess) {
            let _ = std::fs::write(self.cfg_dir.join("session.json"), j);
        }
    }

    fn restore_session(&mut self) {
        let Ok(j) = std::fs::read_to_string(self.cfg_dir.join("session.json")) else { return };
        #[derive(serde::Deserialize)]
        struct Sd {
            path: String,
            line: usize,
            col: usize,
        }
        #[derive(serde::Deserialize)]
        struct Sess {
            active: usize,
            docs: Vec<Sd>,
        }
        let Ok(s) = serde_json::from_str::<Sess>(&j) else { return };
        for d in &s.docs {
            let p = PathBuf::from(&d.path);
            if p.exists() {
                self.open_path(&p);
            }
        }
        if self.docs.is_empty() {
            return;
        }
        let ai = s.active.min(self.docs.len() - 1);
        self.active = Some(ai);
        if let Some(d) = s.docs.get(ai) {
            let text = self.docs[ai].text.clone();
            let cc = crate::util::line_col_to_char(&text, d.line, d.col);
            let id = egui::Id::new(format!("rea_editor_{ai}"));
            let mut st =
                egui::widgets::text_edit::TextEditState::load(&self.ctx, id).unwrap_or_default();
            st.cursor.set_char_range(Some(egui::text::CCursorRange::one(
                egui::text::CCursor::new(cc),
            )));
            st.store(&self.ctx, id);
            self.cursor = Some((d.line, d.col));
            self.cursor_line_no = Some(d.line);
            self.pending_scroll = Some(d.line);
        }
    }

    /// File menu — reopen the tabs saved at last exit
    pub fn restore_last_session(&mut self) {
        self.restore_session();
        if self.docs.is_empty() {
            self.set_status("No saved session found");
        } else {
            self.set_status("Last session restored");
        }
    }

    // ================================================================ v1.9: update check

    pub const UPDATE_URL: &'static str =
        "https://raw.githubusercontent.com/rea-studio/rea-studio/main/latest.json";

    /// Silent background probe (curl, 8s cap — the same strategy the REST
    /// client uses; no extra TLS dependency). Never blocks the UI.
    pub fn upd_start(&mut self, url: Option<String>) {
        if self.upd != UpdState::Idle {
            return;
        }
        self.upd = UpdState::Checking;
        let url = url
            .or_else(|| self.update_url.clone())
            .unwrap_or_else(|| Self::UPDATE_URL.to_string());
        let (tx, rx) = std::sync::mpsc::channel();
        self.upd_rx = Some(rx);
        std::thread::spawn(move || {
            let out = std::process::Command::new("curl")
                .args(["-sL", "--max-time", "8", &url])
                .output();
            let body = out
                .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
                .unwrap_or_default();
            let _ = tx.send(body);
        });
    }

    pub fn upd_check_now(&mut self) {
        self.upd = UpdState::Idle;
        self.upd_start(None);
        self.set_status("Checking for updates…");
    }

    /// Non-blocking poll — called every frame from update().
    pub fn upd_poll(&mut self) {
        let Some(rx) = &self.upd_rx else { return };
        let Ok(body) = rx.try_recv() else { return };
        self.upd_rx = None;
        #[derive(serde::Deserialize)]
        struct Manifest {
            version: String,
            #[serde(default)]
            url: String,
            #[serde(default)]
            notes: String,
        }
        self.upd = match serde_json::from_str::<Manifest>(&body).ok() {
            Some(m) if version_newer(&m.version, env!("CARGO_PKG_VERSION")) => {
                self.set_status(format!("Update available: v{} (Help menu)", m.version));
                UpdState::Available { ver: m.version, url: m.url, notes: m.notes }
            }
            Some(_) => UpdState::UpToDate,
            None => UpdState::Failed,
        };
    }

    pub fn open_url(&self, url: &str) {
        #[cfg(windows)]
        {
            let _ = std::process::Command::new("cmd")
                .args(["/c", "start", "", url])
                .spawn();
        }
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("open").arg(url).spawn();
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let _ = std::process::Command::new("xdg-open").arg(url).spawn();
        }
    }

    // ================================================================ v1.9: outline

    /// Re-scan the active doc for symbols — only when it actually changed.
    pub fn outline_refresh(&mut self) {
        let Some(i) = self.active else { return };
        let (ext, epoch, text) = {
            let d = &self.docs[i];
            let ext = d
                .path
                .as_ref()
                .map(|p| {
                    p.extension()
                        .map(|e| e.to_string_lossy().to_lowercase())
                        .unwrap_or_default()
                })
                .unwrap_or_default();
            (ext, d.lsp_version, d.text.clone())
        };
        let sig = (i, epoch, ext.clone());
        if sig == self.outline_sig {
            return;
        }
        self.outline_items = crate::outline::scan(&ext, &text);
        self.outline_sig = sig;
    }

    fn resolve_theme(&self, key: &ThemeKey) -> Theme {
        match key {
            ThemeKey::Dark => theme::dark(),
            ThemeKey::Light => theme::light(),
            ThemeKey::Custom(p) => std::fs::read_to_string(p)
                .ok()
                .and_then(|s| theme::from_json(&s, &theme::dark()).ok())
                .unwrap_or_else(theme::dark),
        }
    }

    pub fn set_theme_key(&mut self, key: ThemeKey) {
        if key == self.theme_key && self.anim.is_none() {
            return;
        }
        let target = self.resolve_theme(&key);
        self.anim = Some((self.theme_now.clone(), target, Instant::now()));
        self.theme_key = key;
        self.save_settings();
    }

    pub fn toggle_theme(&mut self) {
        let next = if self.theme_now.dark {
            ThemeKey::Light
        } else {
            ThemeKey::Dark
        };
        self.set_theme_key(next);
    }

    // ================================================================ helpers: fonts

    fn load_and_apply_font(&self, fname: &str) -> bool {
        let p = self.cfg_dir.join("fonts").join(fname);
        match std::fs::read(&p) {
            Ok(bytes) => {
                let mut fd = egui::FontDefinitions::default();
                let id = format!("user-{fname}");
                fd.font_data
                    .insert(id.clone(), egui::FontData::from_owned(bytes));
                if let Some(fam) = fd.families.get_mut(&egui::FontFamily::Proportional) {
                    fam.insert(0, id);
                }
                self.ctx.set_fonts(fd);
                true
            }
            Err(_) => false,
        }
    }

    pub fn apply_font(&mut self, fname: &str) {
        if self.load_and_apply_font(fname) {
            self.custom_font = Some(fname.to_string());
            self.set_status(format!("Font applied: {fname}"));
        } else {
            self.set_status(format!("Could not load font: {fname}"));
        }
        self.save_settings();
    }

    pub fn use_default_font(&mut self) {
        self.ctx.set_fonts(egui::FontDefinitions::default());
        self.custom_font = None;
        self.set_status("Reverted to default font");
        self.save_settings();
    }

    pub fn reload_font_list(&mut self) {
        let mut list = Vec::new();
        if let Ok(rd) = std::fs::read_dir(self.cfg_dir.join("fonts")) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                let low = name.to_lowercase();
                if low.ends_with(".ttf") || low.ends_with(".otf") {
                    list.push(name);
                }
            }
        }
        list.sort();
        self.font_list = list;
    }

    pub fn install_font_dialog(&mut self) {
        let Some(picked) = rfd::FileDialog::new()
            .add_filter("Font files", &["ttf", "otf"])
            .pick_file()
        else {
            return;
        };
        let Some(name) = picked.file_name().map(|s| s.to_string_lossy().to_string()) else {
            return;
        };
        let dest = self.cfg_dir.join("fonts").join(&name);
        match std::fs::copy(&picked, &dest) {
            Ok(_) => {
                self.reload_font_list();
                self.apply_font(&name);
            }
            Err(e) => self.set_status(format!("Install failed: {e}")),
        }
    }

    pub fn uninstall_font(&mut self, fname: &str) {
        let p = self.cfg_dir.join("fonts").join(fname);
        let was_active = self.custom_font.as_deref() == Some(fname);
        if std::fs::remove_file(&p).is_ok() || !p.exists() {
            self.reload_font_list();
            if was_active {
                // fall back to program default font
                self.ctx.set_fonts(egui::FontDefinitions::default());
                self.custom_font = None;
                self.set_status("Font removed — reverted to default font");
            } else {
                self.set_status(format!("Font removed: {fname}"));
            }
            self.save_settings();
        } else {
            self.set_status(format!("Could not remove: {fname}"));
        }
    }

    // ================================================================ helpers: themes ext

    pub fn themes_dir(&self) -> PathBuf {
        self.cfg_dir.join("themes")
    }

    pub fn reload_user_themes(&mut self) {
        let mut list = Vec::new();
        if let Ok(rd) = std::fs::read_dir(self.cfg_dir.join("themes")) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().map(|x| x == "json").unwrap_or(false) {
                    if let Ok(s) = std::fs::read_to_string(&p) {
                        if let Ok(th) = theme::from_json(&s, &theme::dark()) {
                            list.push(UserTheme { path: p, theme: th });
                        }
                    }
                }
            }
        }
        list.sort_by(|a, b| a.theme.name.cmp(&b.theme.name));
        self.user_themes = list;
    }

    pub fn install_theme_dialog(&mut self) {
        let Some(picked) = rfd::FileDialog::new()
            .add_filter("REA theme (.json)", &["json"])
            .pick_file()
        else {
            return;
        };
        match std::fs::read_to_string(&picked) {
            Ok(s) => match theme::from_json(&s, &theme::dark()) {
                Ok(_) => {
                    let name = picked
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| "theme.json".to_string());
                    let dest = self.cfg_dir.join("themes").join(&name);
                    match std::fs::copy(&picked, &dest) {
                        Ok(_) => {
                            self.reload_user_themes();
                            self.set_status(format!("Theme installed: {name}"));
                        }
                        Err(e) => self.set_status(format!("Install failed: {e}")),
                    }
                }
                Err(e) => self.set_status(format!("Invalid theme file: {e}")),
            },
            Err(e) => self.set_status(format!("Read failed: {e}")),
        }
    }

    pub fn uninstall_theme(&mut self, p: &Path) {
        let was_active =
            matches!(&self.theme_key, ThemeKey::Custom(cp) if cp.as_path() == p);
        if std::fs::remove_file(p).is_ok() || !p.exists() {
            self.reload_user_themes();
            if was_active {
                self.set_theme_key(ThemeKey::Dark);
            }
            self.set_status("Theme uninstalled");
        } else {
            self.set_status("Could not remove theme");
        }
    }

    // ================================================================ helpers: status

    pub fn set_status(&mut self, s: impl Into<String>) {
        self.status = (s.into(), Some(Instant::now()));
    }

    pub fn status_text(&self) -> String {
        match self.status {
            (ref s, Some(t)) if t.elapsed() < Duration::from_secs(4) => s.clone(),
            _ => String::new(),
        }
    }

    // ================================================================ documents

    pub fn new_file(&mut self) {
        self.untitled_counter += 1;
        let name = format!("Untitled-{}", self.untitled_counter);
        self.docs.push(Doc::new(None, name, String::new()));
        self.active = Some(self.docs.len() - 1);
        self.find.open = false;
        self.multi = None;
    }

    pub fn open_path(&mut self, p: &Path) {
        // v1.7: probe for an LSP server the first time this type is opened
        self.lsp_autodetect(&p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase());
        // already open?
        if let Some(i) = self.docs.iter().position(|d| {
            d.path
                .as_ref()
                .map(|dp| dp == p)
                .unwrap_or(false)
        }) {
            self.active = Some(i);
            return;
        }
        match std::fs::read(p) {
            Ok(bytes) => {
                // decode with BOM / UTF-16 / CRLF handling (#17)
                let (text, bom, crlf) = crate::util::decode_text(&bytes);
                if text.contains('\0') {
                    self.set_status("Binary file — not supported");
                    return;
                }
                let name = p
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| p.display().to_string());
                let mut doc = Doc::new(Some(p.to_path_buf()), name, text.clone());
                doc.bom = bom;
                doc.crlf = crlf;

                // cross-session undo history + crash recovery
                let key = doc.backup_key();
                if let Some(mut st) = UndoStack::load(&self.cfg_dir.join("backups"), &key) {
                    if let Some(last) = st.newest_text() {
                        if last != text {
                            // we have a newer unsaved buffer — offer recovery
                            doc.recovered = Some(last.to_string());
                        }
                    }
                    doc.undo = st;
                }

                self.docs.push(doc);
                self.active = Some(self.docs.len() - 1);
                self.push_recent(p.to_path_buf());
                self.find.open = false;
                self.multi = None;
                self.set_status(format!("Opened {}", p.display()));
            }
            Err(e) => self.set_status(format!("Cannot open: {e}")),
        }
        // register with LSP if a server is configured for this ext
        self.lsp_sync_open();
    }

    /// v1.7: auto-detect well-known language servers the first time a file
    /// type is opened. Only inserts the entry when the binary exists on PATH.
    pub fn lsp_autodetect(&mut self, ext: &str) {
        use std::process::Command;
        if ext.is_empty() || self.settings.lsp.contains_key(ext) || self.settings.lsp_probed.contains(ext) {
            return;
        }
        self.settings.lsp_probed.insert(ext.to_string());
        const CANDIDATES: &[(&str, &str)] = &[
            ("rs", "rust-analyzer"),
            ("py", "pylsp"),
            ("go", "gopls"),
            ("c", "clangd"),
            ("cpp", "clangd"),
            ("h", "clangd"),
            ("hpp", "clangd"),
            ("lua", "lua-language-server"),
            ("zig", "zls"),
            ("ts", "typescript-language-server"),
            ("js", "typescript-language-server"),
            ("php", "intelephense"),
            ("rb", "ruby-lsp"),
            ("kt", "kotlin-language-server"),
            ("fs", "fsautocomplete"),
            ("cs", "OmniSharp"),
        ];
        for (e, cmd) in CANDIDATES {
            if *e == ext {
                let found = Command::new(if cfg!(windows) { "where" } else { "which" })
                    .arg(cmd)
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false);
                if found {
                    self.settings.lsp.insert(ext.to_string(), cmd.to_string());
                    self.save_settings();
                    self.set_status(format!("Language server detected: {cmd} (.{ext})"));
                }
                break;
            }
        }
    }

    // ================================================================ v1.7: split editor

    pub fn toggle_split(&mut self) {
        self.split_on = !self.split_on;
        if self.split_on && self.split_doc.is_none() {
            // default: the next open tab (or the first one)
            if let Some(i) = self.active {
                if self.docs.len() > 1 {
                    self.split_doc = Some((i + 1) % self.docs.len());
                } else {
                    self.split_doc = Some(i);
                }
            }
        }
        self.set_status(if self.split_on {
            "Split editor ON — pick the right-side file in the split bar"
        } else {
            "Split editor off"
        });
    }

    pub fn open_file_dialog(&mut self) {
        let Some(p) = rfd::FileDialog::new().pick_file() else {
            return;
        };
        self.open_path(&p);
    }

    pub fn open_folder_dialog(&mut self) {
        let Some(p) = rfd::FileDialog::new().pick_folder() else {
            return;
        };
        self.set_workspace(p);
    }

    /// Shared workspace setter (dialog, drag&drop, clone, recent folders).
    pub fn set_workspace(&mut self, p: PathBuf) {
        self.workspace = Some(p.clone());
        self.collapsed.clear();
        self.dir_cache.clear();
        self.refresh_tree();
        // recents (files list keeps folders out — folders have their own list)
        self.push_recent_folder(p.clone());
        // keep every terminal session in sync with the new folder
        // (per-shell syntax: PowerShell / cmd / bash)
        let ps_cd = format!(
            "Set-Location -LiteralPath '{}'",
            p.display().to_string().replace('\'', "''")
        );
        #[cfg(windows)]
        let cmd_cd = format!("cd /d \"{}\"", p.display());
        let sh_cd = format!("cd '{}'", p.display().to_string().replace('\'', "'\\''"));
        for s in self.terms.iter_mut() {
            match &mut s.backend {
                crate::terminal::Backend::Real(c) => {
                    #[cfg(windows)]
                    match s.kind {
                        crate::terminal::ShellKind::PowerShell => {
                            c.write_input(&format!("{ps_cd}\r"))
                        }
                        crate::terminal::ShellKind::Cmd => c.write_input(&format!("{cmd_cd}\r")),
                        crate::terminal::ShellKind::GitBash => c.write_input(&format!("{sh_cd}\r")),
                    }
                    // v1.8: unix real-pty sessions all run a POSIX shell
                    #[cfg(not(windows))]
                    c.write_input(&format!("{sh_cd}\r"));
                }
                crate::terminal::Backend::Legacy(tp) => {
                    let _ = tp.send_raw(&ps_cd);
                    let _ = tp.send_raw(crate::terminal::MARKER_CMD);
                }
            }
        }
        self.set_status(format!("Folder opened: {}", p.display()));
    }

    pub fn save(&mut self) {
        let Some(i) = self.active else {
            self.set_status("No file to save");
            return;
        };
        // v1.7: format before writing when format-on-save is enabled
        if self.settings.format_on_save {
            let (ext, name) = {
                let d = &self.docs[i];
                (d.ext(), d.name.clone())
            };
            if let Some(tool) = crate::formatter::tool_for(&ext, &name) {
                if self.fmt_available(tool.name) {
                    crate::formatter::format_active(self);
                }
            }
        }
        if self.docs[i].path.is_some() {
            let p = self.docs[i].path.clone().unwrap();
            // BOM + CRLF restored on disk (#17)
            let text = self.docs[i].save_text();
            match std::fs::write(&p, &text) {
                Ok(_) => {
                    self.docs[i].dirty = false;
                    self.docs[i].conflict = None;
                    self.docs[i].recovered = None;
                    if let Ok(m) = std::fs::metadata(&p) {
                        self.docs[i].disk_mtime = m.modified().ok();
                        self.docs[i].disk_len = m.len();
                    }
                    // persist undo history after each save
                    let bdir = self.cfg_dir.join("backups");
                    let bkey = self.docs[i].backup_key();
                    self.docs[i].undo.persist(&bdir, &bkey);
                    // notify the language server (#7: didSave was never sent)
                    self.lsp_notify_save(i);
                    self.set_status(format!("Saved {}", p.display()));
                }
                Err(e) => self.set_status(format!("Save failed: {e}")),
            }
        } else {
            self.save_as_dialog();
        }
    }

    /// Save every open document that has a path + unsaved changes (#12).
    pub fn save_all(&mut self) {
        let idxs: Vec<usize> = (0..self.docs.len())
            .filter(|&i| self.docs[i].dirty && self.docs[i].path.is_some())
            .collect();
        if idxs.is_empty() {
            self.set_status("Nothing to save");
            return;
        }
        let mut saved = 0;
        for i in idxs {
            let p = self.docs[i].path.clone().unwrap();
            let text = self.docs[i].save_text();
            if std::fs::write(&p, &text).is_ok() {
                self.docs[i].dirty = false;
                self.docs[i].conflict = None;
                if let Ok(m) = std::fs::metadata(&p) {
                    self.docs[i].disk_mtime = m.modified().ok();
                    self.docs[i].disk_len = m.len();
                }
                let bdir = self.cfg_dir.join("backups");
                let bkey = self.docs[i].backup_key();
                self.docs[i].undo.persist(&bdir, &bkey);
                self.lsp_notify_save(i);
                saved += 1;
            }
        }
        self.set_status(format!("Saved {saved} file(s)"));
    }

    /// Ask to close a tab — dirty docs get a confirmation modal (#12).
    pub fn request_close_doc(&mut self, i: usize) {
        if i < self.docs.len() && self.docs[i].dirty {
            self.pending_close = Some(i);
        } else {
            self.close_doc(i);
        }
    }

    /// Window close requested — dirty docs get a confirmation modal (#12).
    pub fn request_exit(&mut self) {
        let any_dirty = self.docs.iter().any(|d| d.dirty);
        if any_dirty {
            self.exit_confirm = true;
        } else {
            self.exit_allow = true;
            self.ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    pub fn save_as_dialog(&mut self) {
        let Some(i) = self.active else {
            return;
        };
        let suggested = self.docs[i].name.clone();
        let Some(p) = rfd::FileDialog::new()
            .set_file_name(&suggested)
            .save_file()
        else {
            return;
        };
        let name = p
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| p.display().to_string());
        self.docs[i].name = name;
        self.docs[i].path = Some(p.clone());
        let text = self.docs[i].save_text();
        match std::fs::write(&p, &text) {
            Ok(_) => {
                self.docs[i].dirty = false;
                self.docs[i].conflict = None;
                self.docs[i].recovered = None;
                if let Ok(m) = std::fs::metadata(&p) {
                    self.docs[i].disk_mtime = m.modified().ok();
                    self.docs[i].disk_len = m.len();
                }
                let bdir = self.cfg_dir.join("backups");
                let bkey = self.docs[i].backup_key();
                self.docs[i].undo.persist(&bdir, &bkey);
                self.push_recent(p.clone());
                self.set_status(format!("Saved {}", p.display()));
            }
            Err(e) => self.set_status(format!("Save failed: {e}")),
        }
    }

    pub fn close_doc(&mut self, i: usize) {
        if i < self.docs.len() {
            // persist undo history so it survives closing AND reopening
            let bdir = self.cfg_dir.join("backups");
            let bkey = self.docs[i].backup_key();
            self.docs[i].undo.persist(&bdir, &bkey);
            if let Some(p) = self.docs[i].path.clone() {
                crate::lsp::set_doc_text(&p, None);
                if let Some(srv) = self.lsp.servers.get(&self.docs[i].ext()).cloned() {
                    crate::lsp::did_close(&srv, &p);
                }
                self.diags.remove(&p);
            }
            self.docs.remove(i);
            if self.docs.is_empty() {
                self.active = None;
            } else if self.active.map(|a| a >= self.docs.len()).unwrap_or(true) {
                self.active = Some(self.docs.len() - 1);
            }
            self.find.open = false;
            self.multi = None;
        }
    }

    pub fn close_all_docs(&mut self) {
        self.docs.clear();
        self.active = None;
        self.find.open = false;
    }

    fn push_recent(&mut self, p: PathBuf) {
        self.recents.retain(|x| x != &p);
        self.recents.insert(0, p);
        self.recents.truncate(12);
        self.save_settings();
    }

    fn push_recent_folder(&mut self, p: PathBuf) {
        self.recent_folders.retain(|x| x != &p);
        self.recent_folders.insert(0, p);
        self.recent_folders.truncate(10);
        self.save_settings();
    }

    // ================================================================ workspace tree

    pub fn dir_list(&mut self, dir: &Path) -> Vec<crate::bars::Node> {
        if let Some(v) = self.dir_cache.get(dir) {
            return v.clone();
        }
        let mut dirs: Vec<crate::bars::Node> = Vec::new();
        let mut files: Vec<crate::bars::Node> = Vec::new();
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if !self.settings.show_dotfiles && name.starts_with('.') {
                    continue;
                }
                let p = e.path();
                let is_dir = p.is_dir();
                let ext = if is_dir {
                    String::new()
                } else {
                    p.extension()
                        .map(|x| x.to_string_lossy().to_lowercase())
                        .unwrap_or_default()
                };
                let n = crate::bars::Node { is_dir, name, path: p, ext };
                if is_dir {
                    dirs.push(n);
                } else {
                    files.push(n);
                }
            }
        }
        dirs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        dirs.truncate(400);
        files.truncate(400);
        let mut all = dirs;
        all.extend(files);
        self.dir_cache.insert(dir.to_path_buf(), all.clone());
        // remember the dir's mtime so terminal-made changes are detected (#14)
        if let Ok(m) = std::fs::metadata(dir) {
            self.dir_mtimes.insert(dir.to_path_buf(), m.modified().unwrap_or(std::time::UNIX_EPOCH));
        }
        all
    }

    /// Invalidate the cache for one dir (and its parents up to the workspace
    /// root) + refresh the file index. Called after every file operation.
    pub fn invalidate_dir(&mut self, dir: &Path) {
        self.dir_cache.remove(dir);
        let mut cur = Some(dir);
        while let Some(c) = cur {
            self.dir_cache.remove(c);
            cur = c.parent();
            if self.workspace.as_ref().map(|w| c == w).unwrap_or(false) {
                break;
            }
        }
        self.dir_mtimes.remove(dir);
        self.refresh_tree();
    }

    /// Poll cached dirs for mtime changes — catches files created or deleted
    /// from the integrated TERMINAL, which previously never refreshed (#14).
    fn check_dir_changes(&mut self) {
        if self.workspace.is_none() || self.last_dir_check.elapsed() < Duration::from_millis(1500) {
            return;
        }
        self.last_dir_check = Instant::now();
        let keys: Vec<PathBuf> = self.dir_mtimes.keys().cloned().collect();
        let mut changed = false;
        for k in keys {
            match std::fs::metadata(&k).and_then(|m| m.modified()) {
                Ok(t) => {
                    let old = self.dir_mtimes.get(&k).copied().unwrap_or(std::time::UNIX_EPOCH);
                    if t != old {
                        self.dir_mtimes.insert(k.clone(), t);
                        self.dir_cache.remove(&k);
                        changed = true;
                    }
                }
                Err(_) => {
                    // dir was deleted
                    self.dir_mtimes.remove(&k);
                    self.dir_cache.remove(&k);
                    changed = true;
                }
            }
        }
        if changed {
            self.ws_files.clear();
            if let Some(ws) = self.workspace.clone() {
                self.walk_ws(&ws, 0);
            }
        }
    }

    pub fn refresh_tree(&mut self) {
        self.dir_cache.clear();
        self.ws_files.clear();
        if let Some(ws) = self.workspace.clone() {
            self.walk_ws(&ws, 0);
        }
    }

    fn walk_ws(&mut self, dir: &Path, depth: usize) {
        // raised caps (#15): depth 10 / 20k files so big workspaces are indexed
        if depth > 10 || self.ws_files.len() > 20_000 {
            return;
        }
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if !self.settings.show_dotfiles && name.starts_with('.') {
                    continue;
                }
                let p = e.path();
                if p.is_dir() {
                    let low = name.to_lowercase();
                    if matches!(
                        low.as_str(),
                        "node_modules" | "target" | "dist" | "build" | "__pycache__" | ".git"
                    ) {
                        continue;
                    }
                    self.walk_ws(&p, depth + 1);
                } else {
                    self.ws_files.push(p);
                    if self.ws_files.len() > 20_000 {
                        return;
                    }
                }
            }
        }
    }

    pub fn toggle_explorer(&mut self) {
        if self.show_sidebar && self.panel_mode == PanelMode::Explorer {
            self.show_sidebar = false;
        } else {
            self.panel_mode = PanelMode::Explorer;
            self.show_sidebar = true;
        }
    }

    pub fn click_panel(&mut self, mode: PanelMode) {
        if self.show_sidebar && self.panel_mode == mode {
            self.show_sidebar = false;
        } else {
            self.panel_mode = mode;
            self.show_sidebar = true;
            if mode == PanelMode::Search {
                self.refresh_tree();
            }
        }
    }

    // ================================================================ palette

    pub fn open_palette(&mut self, mode: PaletteMode) {
        let mut pal = Palette::new(mode);
        match mode {
            PaletteMode::Commands => {
                pal.items = vec![
                    cmd("new_file", "New File", "Ctrl+N"),
                    cmd("open_file", "Open File…", "Ctrl+O"),
                    cmd("open_folder", "Open Folder…", "Ctrl+Shift+O"),
                    cmd("github_clone", "GitHub: Clone Repository…", "Ctrl+Shift+G"),
                    cmd("save", "Save", "Ctrl+S"),
                    cmd("save_as", "Save As…", "Ctrl+Shift+S"),
                    cmd("save_all", "Save All", ""),
                    cmd("find", "Find in File", "Ctrl+F"),
                    cmd("replace", "Find & Replace", "Ctrl+H"),
                    cmd("find_in_files", "Search in Workspace…", "Ctrl+Shift+F"),
                    cmd("undo", "Undo", "Ctrl+Z"),
                    cmd("redo", "Redo", "Ctrl+Y"),
                    cmd("multi_next", "Multi-cursor: Add Next Occurrence", "Ctrl+D"),
                    cmd("multi_all", "Multi-cursor: Select All Occurrences", "Ctrl+Shift+L"),
                    cmd("toggle_comment", "Toggle Line Comment", "Ctrl+/"),
                    cmd("goto_line", "Go to Line…", "Ctrl+G"),
                    cmd("goto_definition", "Go to Definition (LSP)", "F12"),
                    cmd("start_debug", "Start Debugging", "F5"),
                    cmd("stop_debug", "Stop Debugging", "Shift+F5"),
                    cmd("toggle_breakpoint", "Toggle Breakpoint", "F9"),
                    cmd("install_vsix", "Extensions: Install from .vsix…", ""),
                    cmd("toggle_wrap", "Toggle Word Wrap", "Alt+Z"),
                    cmd("toggle_dotfiles", "Toggle Hidden Files (dotfiles)", ""),
                    cmd("toggle_autosave", "Toggle Auto-save", ""),
                    cmd("toggle_breadcrumbs", "Toggle Breadcrumbs", ""),
                    cmd("settings", "Open Settings", "Ctrl+,") ,
                    cmd("toggle_theme", "Toggle Theme (Dark+ / Google Light)", ""),
                    cmd("toggle_terminal", "Toggle Terminal", "Ctrl+`"),
                    cmd("new_terminal", "Terminal: New Session", ""),
                    cmd("new_terminal_cmd", "Terminal: New cmd Session", ""),
                    cmd("restart_terminal", "Terminal: Restart Session", ""),
                    cmd("git_panel", "Git: Show Panel", ""),
                    cmd("git_refresh", "Git: Refresh Status", ""),
                    cmd("git_stage_all", "Git: Stage All Changes", ""),
                    cmd("format_doc", "Format Document (rustfmt/black/prettier/gofmt)", "Alt+Shift+F"),
                    cmd("split_editor", "Toggle Split Editor", ""),
                    cmd("toggle_minimap", "Toggle Minimap", ""),
                    cmd("open_browser", "Open in Default Browser", "Alt+B"),
                    cmd("live_start", "Live Server: Start (auto-reload)", "Alt+L"),
                    cmd("live_stop", "Live Server: Stop", ""),
                    cmd("rest_send", "REST Client: Send Request", "Alt+R"),
                    cmd("rest_panel", "REST Client: Show Panel", ""),
                    cmd("recco_show", "Code Recommendations: Show (this language)", "Ctrl+Space"),
                    cmd("plugins", "Plugins: Manage", ""),
                    cmd("toggle_explorer", "Toggle Explorer", "Ctrl+B"),
                    cmd("bookmark_toggle", "Toggle Bookmark", "Ctrl+F2"),
                    cmd("bookmark_next", "Next Bookmark", "F2"),
                    cmd("bookmark_prev", "Previous Bookmark", "Shift+F2"),
                    cmd("bookmark_clear", "Clear Bookmarks (this file)", "Alt+F2"),
                    cmd("zoom_in", "Zoom In (editor text)", "Ctrl+="),
                    cmd("zoom_out", "Zoom Out (editor text)", "Ctrl+-"),
                    cmd("zoom_reset", "Reset Zoom", "Ctrl+0"),
                    // ---- v2.0: macro + column editing ----
                    cmd("macro_record", "Macro: Start/Stop Recording", "Ctrl+Shift+R"),
                    cmd("macro_play", "Macro: Play Recorded Macro", "Ctrl+Shift+M"),
                    cmd("col_select", "Column Selection (block mode)", "Alt+Shift+Arrows"),
                    // ---- v2.1: tools / diff / todo ----
                    cmd("diff_files", "Tools: Compare Two Files…", ""),
                    cmd("todo_panel", "Tools: TODO Scanner (Workspace)", ""),
                    cmd("doc_stats", "Tools: Document Statistics", ""),
                    cmd("tool_sort_asc", "Tools: Sort Lines A→Z", ""),
                    cmd("tool_sort_desc", "Tools: Sort Lines Z→A", ""),
                    cmd("tool_dedupe", "Tools: Remove Duplicate Lines", ""),
                    cmd("tool_rm_empty", "Tools: Remove Empty Lines", ""),
                    cmd("tool_reverse", "Tools: Reverse Line Order", ""),
                    cmd("tool_trim_trail", "Tools: Trim Trailing Spaces", ""),
                    cmd("tool_trim_lead", "Tools: Trim Leading Spaces", ""),
                    cmd("tool_upper", "Tools: UPPERCASE Selection", ""),
                    cmd("tool_lower", "Tools: lowercase Selection", ""),
                    cmd("tool_title", "Tools: Title Case Selection", ""),
                    cmd("tool_invert", "Tools: iNVERT cASE Selection", ""),
                    cmd("tool_b64_enc", "Tools: Base64 Encode", ""),
                    cmd("tool_b64_dec", "Tools: Base64 Decode", ""),
                    cmd("tool_url_enc", "Tools: URL Encode", ""),
                    cmd("tool_url_dec", "Tools: URL Decode", ""),
                    cmd("tool_json_esc", "Tools: JSON Escape Selection", ""),
                    cmd("tool_json_unesc", "Tools: JSON Unescape Selection", ""),
                    cmd("tool_md5", "Tools: MD5 Hash (selection/word)", ""),
                    cmd("tool_sha1", "Tools: SHA-1 Hash (selection/word)", ""),
                    cmd("tool_sha256", "Tools: SHA-256 Hash (selection/word)", ""),
                    cmd("tool_timestamp", "Tools: Insert Timestamp", ""),
                    cmd("eol_lf", "Tools: EOL → Unix (LF)", ""),
                    cmd("eol_crlf", "Tools: EOL → Windows (CRLF)", ""),
                    cmd("bom_toggle", "Tools: Toggle UTF-8 BOM", ""),
                    cmd("outline", "Toggle Function List (Outline)", ""),
                    cmd("restore_session", "File: Restore Last Session", ""),
                    cmd("check_updates", "Help: Check for Updates", ""),
                    cmd("extensions", "Extensions: Themes & Fonts", ""),
                    cmd("close_all", "Close All Files (Welcome)", ""),
                    cmd("about", "About REA Studio", ""),
                ];
            }
            PaletteMode::Files => {
                for p in self.recent_folders.clone() {
                    let name = p
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| p.display().to_string());
                    pal.items.push(PalItem {
                        id: format!("openfolder:{}", p.display()),
                        label: format!("📁 {name}"),
                        hint: p.display().to_string(),
                    });
                }
                for p in self.recents.clone() {
                    let name = p
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| p.display().to_string());
                    let dir = p
                        .parent()
                        .map(|x| x.display().to_string())
                        .unwrap_or_default();
                    pal.items.push(PalItem {
                        id: format!("open:{}", p.display()),
                        label: name,
                        hint: dir,
                    });
                }
                let ws = self.ws_files.clone();
                for p in ws {
                    let name = p
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default();
                    if pal.items.iter().any(|it| {
                        it.id == format!("open:{}", p.display())
                    }) {
                        continue;
                    }
                    let dir = p
                        .parent()
                        .map(|x| x.display().to_string())
                        .unwrap_or_default();
                    pal.items.push(PalItem {
                        id: format!("open:{}", p.display()),
                        label: name,
                        hint: dir,
                    });
                }
            }
            // GotoLine items are built in open_goto_line(); nothing here
            PaletteMode::GotoLine => {}
        }
        pal.refilter();
        self.palette = Some(pal);
        // make sure the find box / suggestion popup does not steal the focus meanwhile
        self.find.focus_req = false;
        self.suggest = None;
    }

    // ================================================================ find & replace

    pub fn open_find(&mut self) {
        if self.active.is_none() {
            self.set_status("Open a file first to use Find");
            return;
        }
        self.find.open = true;
        self.find.focus_req = true;
        self.palette = None;
        self.suggest = None;
        self.find_maybe_recompute();
    }

    pub fn close_find(&mut self) {
        self.find.open = false;
    }

    /// Build the matcher for the current find options.
    fn find_matcher(&self) -> Result<crate::editops::Matcher2, String> {
        let q = self.find.query.clone();
        if q.is_empty() {
            return Err("empty".into());
        }
        if self.find.use_regex {
            let re = regex::RegexBuilder::new(&q)
                .case_insensitive(!self.find.case_sensitive)
                .build()
                .map_err(|e| e.to_string())?;
            Ok(crate::editops::Matcher2::Regex(re))
        } else if self.find.whole_word {
            let pat = format!(r"\b(?:{})\b", regex::escape(&q));
            let re = regex::RegexBuilder::new(&pat)
                .case_insensitive(!self.find.case_sensitive)
                .build()
                .map_err(|e| e.to_string())?;
            Ok(crate::editops::Matcher2::Regex(re))
        } else {
            Ok(crate::editops::Matcher2::Literal {
                needle: if self.find.case_sensitive {
                    q
                } else {
                    q.to_lowercase()
                },
                cs: self.find.case_sensitive,
            })
        }
    }

    pub fn find_maybe_recompute(&mut self) {
        let Some(i) = self.active else {
            self.find.matches.clear();
            return;
        };
        let sig = (
            self.find.query.clone(),
            i,
            // content fingerprint (len + FNV) instead of lsp_version:
            // lsp_version is only bumped for docs with an LSP server, so for
            // plain files the signature stayed stale and REPLACE-ALL applied
            // old ranges to new text — corrupting the file (#10)
            self.docs[i].text.len(),
            crate::util::fnv(&self.docs[i].text),
            self.find.case_sensitive,
            self.find.whole_word,
            self.find.use_regex,
        );
        if self.find.sig.as_ref() == Some(&sig) {
            return;
        }
        self.find.sig = Some(sig);
        self.find.error = None;
        self.find.matches.clear();
        self.find.idx = 0;
        if self.find.query.is_empty() {
            return;
        }
        let matcher = match self.find_matcher() {
            Ok(m) => m,
            Err(e) => {
                if e != "empty" {
                    self.find.error = Some(e);
                }
                return;
            }
        };
        self.find.matches = matcher.find_all(&self.docs[i].text, 10_000);
    }

    /// scroll target line of the active match
    fn find_current_line(&self) -> Option<usize> {
        let i = self.active?;
        let (s, _) = *self.find.matches.get(self.find.idx)?;
        let (line, _) = crate::util::char_to_line_col(&self.docs[i].text, s);
        Some(line)
    }

    pub fn find_next(&mut self) {
        self.find_maybe_recompute();
        if self.find.matches.is_empty() {
            return;
        }
        self.find.idx = (self.find.idx + 1) % self.find.matches.len();
        self.pending_scroll = self.find_current_line();
    }

    pub fn find_prev(&mut self) {
        self.find_maybe_recompute();
        if self.find.matches.is_empty() {
            return;
        }
        self.find.idx =
            (self.find.idx + self.find.matches.len() - 1) % self.find.matches.len();
        self.pending_scroll = self.find_current_line();
    }

    /// Replace the active match, then jump to the NEXT match (by position —
    /// previously it recomputed and returned to the FIRST match, #10).
    pub fn find_replace_one(&mut self) {
        self.find_maybe_recompute();
        let Some(i) = self.active else { return };
        let Some(&(s, e)) = self.find.matches.get(self.find.idx) else {
            return;
        };
        let text = self.docs[i].text.clone();
        // regex: expand $1..$9 in the replacement
        let with = self.expand_replace(&text, (s, e));
        let (new_text, _caret) = crate::editops::replace_range(&text, (s, e), &with);
        self.docs[i].text = new_text;
        self.docs[i].dirty = true;
        self.docs[i].last_edit = Some(Instant::now());
        let txt_c = self.docs[i].text.clone();
        self.docs[i].undo.force(&txt_c, Some(s));
        self.lsp_notify_change(i);
        // recompute and land on the first match at/after the replaced position
        self.find.sig = None;
        self.find_maybe_recompute();
        self.find.idx = self
            .find
            .matches
            .iter()
            .position(|(ms, _)| *ms >= s)
            .unwrap_or(0);
        self.pending_scroll = self.find_current_line();
    }

    pub fn find_replace_all(&mut self) {
        self.find_maybe_recompute();
        let Some(i) = self.active else { return };
        if self.find.matches.is_empty() {
            return;
        }
        let text = self.docs[i].text.clone();
        let ranges = self.find.matches.clone();
        let mut out = text.clone();
        // apply from last to first so indices stay valid; regex expansion per match
        let sorted_desc = {
            let mut r = ranges.clone();
            r.sort_by(|a, b| b.0.cmp(&a.0));
            r
        };
        for range in sorted_desc {
            let with = self.expand_replace(&out, range);
            let (nt, _) = crate::editops::replace_range(&out, range, &with);
            out = nt;
        }
        let caret = self.cursor_char.unwrap_or(0).min(out.chars().count());
        self.docs[i].text = out;
        self.docs[i].dirty = true;
        self.docs[i].last_edit = Some(Instant::now());
        let txt_c = self.docs[i].text.clone();
        self.docs[i].undo.force(&txt_c, Some(caret));
        self.find.matches.clear();
        self.find.sig = None;
        self.lsp_notify_change(i);
        self.set_status("Replaced all matches");
    }

    /// Expand $1..$9 group references when in regex mode.
    fn expand_replace(&self, text: &str, range: (usize, usize)) -> String {
        if !self.find.use_regex {
            return self.find.replace.clone();
        }
        let Ok(re) = regex::Regex::new(&self.find.query) else {
            return self.find.replace.clone();
        };
        // map char range to byte range
        let byte_start: usize = text.chars().take(range.0).map(|c| c.len_utf8()).sum();
        let byte_len: usize = text
            .chars()
            .skip(range.0)
            .take(range.1 - range.0)
            .map(|c| c.len_utf8())
            .sum();
        if let Some(caps) = re.captures(&text[byte_start..byte_start + byte_len]) {
            let mut out = String::new();
            caps.expand(&self.find.replace, &mut out);
            return out;
        }
        self.find.replace.clone()
    }

    // ================================================================ terminal

    pub fn toggle_terminal(&mut self) {
        self.terminal_visible = !self.terminal_visible;
        if self.terminal_visible {
            self.term_focus_req = true;
        }
        self.save_settings();
    }

    /// Spawn a new terminal session (ConPTY when available).
    pub fn spawn_terminal(&mut self, kind: crate::terminal::ShellKind) {
        self.term_id_counter += 1;
        let id = self.term_id_counter;
        let cwd = self.workspace.clone();
        self.terminal_visible = true;
        self.bottom_tab = BottomTab::Terminal;

        #[cfg(windows)]
        {
            if self.settings.terminal_conpty {
                match crate::conpty::ConPtySession::spawn(
                    kind,
                    cwd.as_deref(),
                    100,
                    24,
                    self.theme_now.text,
                    self.theme_now.editor_bg,
                ) {
                    Ok(s) => {
                        self.terms.push(crate::terminal::TermSession {
                            id,
                            kind,
                            backend: crate::terminal::Backend::Real(Box::new(s)),
                            scroll_off: 0,
                            focused: true,
                            input: String::new(),
                            hist: Vec::new(),
                            hist_idx: None,
                        });
                        self.term_active = self.terms.len() - 1;
                        return;
                    }
                    Err(e) => {
                        self.set_status(format!("ConPTY unavailable ({e}) — using pipe mode"));
                    }
                }
            }
        }
        // v1.8: real POSIX pty on Linux/macOS (falls back to pipe mode)
        #[cfg(unix)]
        {
            match crate::pty::PtySession::spawn(
                kind,
                cwd.as_deref(),
                100,
                24,
                self.theme_now.text,
                self.theme_now.editor_bg,
            ) {
                Ok(s) => {
                    self.terms.push(crate::terminal::TermSession {
                        id,
                        kind,
                        backend: crate::terminal::Backend::Real(Box::new(s)),
                        scroll_off: 0,
                        focused: true,
                        input: String::new(),
                        hist: Vec::new(),
                        hist_idx: None,
                    });
                    self.term_active = self.terms.len() - 1;
                    return;
                }
                Err(e) => {
                    self.set_status(format!("PTY unavailable ({e}) — using pipe mode"));
                }
            }
        }
        self.spawn_terminal_inner(id, kind);
    }

    fn spawn_terminal_inner(&mut self, id: usize, kind: crate::terminal::ShellKind) {
        let cwd = self.workspace.clone();
        match crate::terminal::TerminalProc::spawn(cwd.as_deref()) {
            Ok(tp) => {
                self.terms.push(crate::terminal::TermSession {
                    id,
                    kind,
                    backend: crate::terminal::Backend::Legacy(Box::new(tp)),
                    scroll_off: 0,
                    focused: true,
                    input: String::new(),
                    hist: Vec::new(),
                    hist_idx: None,
                });
                self.term_active = self.terms.len() - 1;
            }
            Err(e) => self.set_status(format!("Terminal failed to start: {e}")),
        }
    }

    /// Spawn a legacy pipe-mode session (menu fallback).
    pub fn spawn_terminal_legacy(&mut self) {
        self.term_id_counter += 1;
        self.spawn_terminal_inner(self.term_id_counter, crate::terminal::ShellKind::PowerShell);
    }

    pub fn ensure_terminal(&mut self) {
        if self.terms.is_empty() {
            self.spawn_terminal(crate::terminal::ShellKind::PowerShell);
            self.term_focus_req = true;
        }
    }

    /// Send a command line to the ACTIVE session.
    pub fn send_terminal(&mut self, cmd: &str) {
        self.ensure_terminal();
        let i = self.term_active.min(self.terms.len().saturating_sub(1));
        if let Some(s) = self.terms.get_mut(i) {
            match &mut s.backend {
                crate::terminal::Backend::Real(c) => {
                    // the shell does its own echo; just push the line
                    c.write_input(&format!("{cmd}\r"));
                    s.scroll_off = 0;
                }
                crate::terminal::Backend::Legacy(tp) => {
                    // echo + marker: the prompt/echo is drawn by US because
                    // PowerShell never writes a prompt to a pipe
                    if !cmd.is_empty() {
                        s.hist.push(cmd.to_string());
                        if s.hist.len() > 100 {
                            s.hist.remove(0);
                        }
                    }
                    s.hist_idx = None;
                    if let Err(e) = tp.send_command(cmd) {
                        self.set_status(format!("Terminal write failed: {e}"));
                    }
                }
            }
        }
    }

    /// Kill + respawn the ACTIVE session.
    pub fn restart_terminal(&mut self) {
        if self.terms.is_empty() {
            self.ensure_terminal();
            return;
        }
        let i = self.term_active.min(self.terms.len() - 1);
        let kind = self.terms[i].kind;
        let id = self.terms[i].id;
        self.terms.remove(i); // Drop kills the old process
        self.term_id_counter += 1;
        let new_id = self.term_id_counter;
        self.term_active = i.min(self.terms.len().saturating_sub(1));
        self.terminal_visible = true;
        let _ = id;
        self.spawn_terminal(kind);
        let _ = new_id;
        self.term_focus_req = true;
        self.set_status("Terminal restarted");
    }

    pub fn clear_terminal(&mut self) {
        let i = self.term_active.min(self.terms.len().saturating_sub(1));
        if let Some(s) = self.terms.get_mut(i) {
            match &mut s.backend {
                crate::terminal::Backend::Real(c) => {
                    c.vt.clear();
                }
                crate::terminal::Backend::Legacy(tp) => tp.clear(),
            }
        }
    }

    pub fn close_terminal(&mut self, i: usize) {
        if i >= self.terms.len() {
            return;
        }
        self.terms.remove(i); // Drop kills the process
        if self.terms.is_empty() {
            self.term_active = 0;
        } else {
            self.term_active = self.term_active.min(self.terms.len() - 1);
        }
    }

    /// cwd label for the tabs row (legacy sessions track cwd; ConPTY shows its
    /// own prompt so we return empty there).
    pub fn active_session_cwd(&self) -> Option<String> {
        self.terms.get(self.term_active).and_then(|s| match &s.backend {
            crate::terminal::Backend::Legacy(tp) => Some(tp.cwd()),
            _ => None,
        })
    }

    pub fn terms_focus_flag(&self) -> bool {
        self.terms.get(self.term_active).map(|s| s.focused).unwrap_or(false)
    }

    // ================================================================ v1.6 plugins

    /// Poll the background REST job (called from update() while busy).
    pub fn rest_poll(&mut self) {
        let done = self
            .rest
            .job
            .as_ref()
            .map(|j| j.lock().unwrap().take())
            .unwrap_or(None);
        if let Some(mut r) = done {
            // merge the request meta into the result
            if let Some(i) = self.active {
                let text = self.docs[i].text.clone();
                let ext = self.docs[i].ext();
                if matches!(ext.as_str(), "http" | "rest") {
                    let reqs = crate::restclient::parse_requests(&text);
                    if let Some(req) = reqs.get(self.rest.sel.min(reqs.len().saturating_sub(1))) {
                        r.name = req.name.clone();
                        r.method = req.method.clone();
                        r.url = req.url.clone();
                    }
                }
            }
            crate::restclient::store_capture(self, &r);
            self.rest.results.push(r);
            if self.rest.results.len() > 20 {
                self.rest.results.remove(0);
            }
            self.rest.busy = false;
        }
    }

    /// Send the currently selected request (Alt+R / palette / button).
    pub fn rest_send_active(&mut self) {
        if !self.plugin_enabled("rest-client") {
            self.set_status("REST Client plugin is disabled (Extensions panel)");
            return;
        }
        let Some(i) = self.active else {
            self.set_status("REST Client: open a .http file first");
            return;
        };
        let ext = self.docs[i].ext();
        if !matches!(ext.as_str(), "http" | "rest") {
            self.set_status("REST Client: active file is not .http / .rest");
            return;
        }
        if self.rest.busy {
            return;
        }
        let text = self.docs[i].text.clone();
        let reqs = crate::restclient::parse_requests(&text);
        if reqs.is_empty() {
            self.set_status("REST Client: no requests found (use  GET https://…)");
            return;
        }
        let sel = self.rest.sel.min(reqs.len() - 1);
        let req = reqs[sel].clone();
        self.terminal_visible = true;
        self.bottom_tab = BottomTab::Rest;
        crate::restclient::send(self, &req);
        self.set_status(format!("REST Client: sending {} {}…", req.method, req.url));
    }

    // ================================================================ code suggestions

    pub fn move_suggest_sel(&mut self, dir: i32) {
        if let Some(sg) = self.suggest.as_mut() {
            let n = sg.total();
            if n > 0 {
                sg.sel = ((sg.sel as i32 + dir).rem_euclid(n as i32)) as usize;
            }
        }
    }

    /// Replace the word at the caret with the selected suggestion.
    /// v1.6: selection may also be a Code Recommendation snippet (◆ rows).
    pub fn accept_suggestion(&mut self, ctx: &egui::Context) {
        let Some(sg) = self.suggest.clone() else { return };
        let Some(i) = self.active else { return };
        let Some(cc) = self.cursor_char else { return };
        let snip = sg.snips.get(sg.sel).cloned();
        let item = if snip.is_some() {
            String::new()
        } else {
            sg.items.get(sg.sel - sg.snips.len()).cloned().unwrap_or_default()
        };
        if i >= self.docs.len() {
            return;
        }
        let chars: Vec<char> = self.docs[i].text.chars().collect();
        let (ws, we) = crate::complete::word_at(&chars, cc);

        // snippet path: cut the typed word, insert the template, put the
        // caret at the @@ marker
        if let Some(s) = snip {
            let (code, caret_in_code) = crate::recco::expand(s.code);
            let code_chars: Vec<char> = code.chars().collect();
            let mut new_text = String::with_capacity(self.docs[i].text.len() + code.len());
            new_text.extend(chars[..ws].iter());
            new_text.extend(code_chars.iter());
            new_text.extend(chars[we..].iter());
            let new_cc = ws + caret_in_code;
            self.docs[i].text = new_text;
            self.docs[i].dirty = true;
            self.cursor_char = Some(new_cc);
            self.suppress_len = Some(self.docs[i].text.len());
            self.suggest = None;
            self.move_caret_to(ctx, i, new_cc);
            self.set_status(format!("Inserted recommendation: {}", s.label));
            return;
        }

        if item.is_empty() {
            return;
        }
        let mut new_text = String::with_capacity(self.docs[i].text.len() + item.len());
        new_text.extend(chars[..ws].iter());
        new_text.push_str(&item);
        new_text.extend(chars[we..].iter());
        let new_cc = ws + item.chars().count();
        self.docs[i].text = new_text;
        self.docs[i].dirty = true;
        self.cursor_char = Some(new_cc);
        self.suppress_len = Some(self.docs[i].text.len());
        self.suggest = None;

        // move the TextEdit caret to the end of the inserted word
        self.move_caret_to(ctx, i, new_cc);
    }

    /// Shared caret move (used by word + snippet acceptance).
    fn move_caret_to(&mut self, ctx: &egui::Context, i: usize, cc: usize) {
        let id = egui::Id::new(format!("rea_editor_{i}"));
        let mut st = egui::widgets::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
        st.cursor.set_char_range(Some(egui::text::CCursorRange::one(
            egui::text::CCursor::new(cc),
        )));
        st.store(ctx, id);
        self.ctx.request_repaint();
    }

    /// Ctrl+Space — force the popup with all keywords + doc words.
    pub fn suggest_force_open(&mut self) {
        if !self.editor_focused || self.suggest_pos.is_none() {
            return;
        }
        let Some(i) = self.active else { return };
        let Some(cc) = self.cursor_char else { return };
        let chars: Vec<char> = self.docs[i].text.chars().collect();
        let (ws, we) = crate::complete::word_at(&chars, cc);
        let base: String = chars[ws..we].iter().collect();
        let sig = (i, chars.len());
        if self.word_cache_sig != sig {
            self.word_cache = crate::complete::doc_words(&self.docs[i].text, 4000);
            self.word_cache_sig = sig;
        }
        let kw = crate::complete::keywords(&self.docs[i].ext());
        let items = crate::complete::filter(kw, &self.word_cache, &base, 24);
        let snips = crate::recco::filter_snips(&self.docs[i].ext(), &base, 6);
        if items.is_empty() && snips.is_empty() {
            return;
        }
        self.suggest = Some(crate::complete::Suggest {
            items,
            sel: 0,
            base,
            pos: self.suggest_pos.unwrap(),
            doc: i,
            snips,
        });
    }

    // ================================================================ v1.4: editing engine

    /// Apply egui text style sizes from settings.
    pub fn apply_font_size(&mut self) {
        let fs = self.settings.font_size;
        if (self.applied_font_size - fs).abs() < f32::EPSILON {
            return;
        }
        self.applied_font_size = fs;
        let mut style = (*self.ctx.style()).clone();
        for (ts, base) in [
            (egui::TextStyle::Body, fs),
            (egui::TextStyle::Monospace, fs),
            (egui::TextStyle::Button, fs),
            (egui::TextStyle::Small, (fs * 0.8).max(9.0)),
            (egui::TextStyle::Heading, fs * 1.6),
        ] {
            if let Some(f) = style.text_styles.get_mut(&ts) {
                f.size = base;
            }
        }
        self.ctx.set_style(style);
        self.save_settings();
    }

    /// Central edit application: undo bookkeeping, dirty flag, caches, LSP.
    pub fn notify_edit(&mut self, i: usize, cursor: Option<usize>) {
        let text = self.docs[i].text.clone();
        self.docs[i].undo.after_edit(&text, cursor);
        self.docs[i].dirty = true;
        self.docs[i].last_edit = Some(Instant::now());
        self.word_cache_sig = (usize::MAX, 0); // invalidate completion word cache
        self.lsp_notify_change(i);
    }

    // ---------------- undo / redo ----------------

    pub fn do_undo(&mut self) {
        let Some(i) = self.active else { return };
        if let Some((text, cursor)) = self.docs[i].undo.undo() {
            self.set_doc_text_undo(i, text, cursor);
            self.set_status("Undo");
        } else {
            self.set_status("Nothing to undo");
        }
    }

    pub fn do_redo(&mut self) {
        let Some(i) = self.active else { return };
        if let Some((text, cursor)) = self.docs[i].undo.redo() {
            self.set_doc_text_undo(i, text, cursor);
            self.set_status("Redo");
        } else {
            self.set_status("Nothing to redo");
        }
    }

    pub fn set_doc_text_undo(&mut self, i: usize, text: String, cursor: Option<usize>) {
        self.docs[i].text = text;
        self.docs[i].dirty = true;
        self.docs[i].last_edit = Some(Instant::now());
        self.word_cache_sig = (usize::MAX, 0);
        self.multi = None;
        if let Some(cc) = cursor {
            let id = egui::Id::new(format!("rea_editor_{i}"));
            let mut st = egui::widgets::text_edit::TextEditState::load(&self.ctx, id).unwrap_or_default();
            st.cursor
                .set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(cc))));
            st.store(&self.ctx, id);
        }
        self.lsp_notify_change(i);
        self.ctx.request_repaint();
    }

    // ---------------- multi-occurrence (Ctrl+D / Ctrl+Shift+L) ----------------

    fn current_word_or_selection(&self, i: usize) -> Option<(usize, usize)> {
        if let Some((s, e)) = self.sel_range {
            if s != e {
                return Some((s.min(e), e.max(s)));
            }
        }
        let cc = self.cursor_char?;
        let chars: Vec<char> = self.docs[i].text.chars().collect();
        let (ws, we) = crate::complete::word_at(&chars, cc);
        if we > ws {
            Some((ws, we))
        } else {
            None
        }
    }

    /// Ctrl+D — add next occurrence of the word/selection to the multi set.
    /// Wrap-around search no longer re-selects an already-selected region
    /// (the old code pushed the FIRST region again on every wrap, #3).
    pub fn multi_select_next(&mut self) {
        let Some(i) = self.active else { return };
        let text = self.docs[i].text.clone();
        let chars: Vec<char> = text.chars().collect();
        if let Some(m) = &mut self.multi {
            if m.doc != i {
                self.multi = None;
            } else {
                // find the next occurrence AFTER the last added region
                let last_end = m.regions.iter().map(|r| r.1).max().unwrap_or(0);
                let needle: String = {
                    let (s, e) = m.regions[0];
                    chars[s.min(chars.len())..e.min(chars.len())].iter().collect()
                };
                if needle.is_empty() {
                    return;
                }
                let low_chars: Vec<char> = needle.to_lowercase().chars().collect();
                let hay: Vec<char> = text.to_lowercase().chars().collect();
                let n = low_chars.len();
                if n == 0 {
                    return;
                }
                let mut found = None;
                let mut j = last_end;
                while j + n <= hay.len() {
                    if hay[j..j + n] == low_chars[..] {
                        let r = (j, j + n);
                        if !m.regions.contains(&r) {
                            found = Some(r);
                            break;
                        }
                    }
                    j += 1;
                }
                // wrap-around search — skipping regions we already have
                if found.is_none() {
                    let mut j = 0;
                    while j + n <= hay.len() && j < last_end {
                        if hay[j..j + n] == low_chars[..] {
                            let r = (j, j + n);
                            if !m.regions.contains(&r) {
                                found = Some(r);
                                break;
                            }
                        }
                        j += 1;
                    }
                }
                if let Some(r) = found {
                    m.regions.push(r);
                    m.regions.sort();
                    m.caret_region = m.regions.iter().position(|x| *x == r).unwrap_or(0);
                    self.set_selection(i, r);
                }
                return;
            }
        }
        // start a new multi set from the current word/selection
        let Some(first) = self.current_word_or_selection(i) else { return };
        self.multi = Some(MultiEdit {
            doc: i,
            regions: vec![first],
            caret_region: 0,
        });
        self.set_status("Multi-cursor: Ctrl+D adds next, Ctrl+Shift+L selects all, Esc exits");
    }

    /// Ctrl+Shift+L — select ALL occurrences.
    pub fn multi_select_all(&mut self) {
        let Some(i) = self.active else { return };
        let Some(first) = self.current_word_or_selection(i) else { return };
        let text = self.docs[i].text.clone();
        let needle: String = crate::util::char_slice(&text, first.0, first.1).to_string();
        let mut regions = crate::editops::find_all(&text, &needle, true, true, 2000);
        if regions.len() <= 1 {
            self.set_status("No other occurrences");
            return;
        }
        regions.sort();
        let n = regions.len();
        self.multi = Some(MultiEdit {
            doc: i,
            regions,
            caret_region: n - 1,
        });
        let last = n - 1;
        if let Some(m) = &self.multi {
            let r = m.regions[last];
            self.set_selection(i, r);
        }
        self.set_status(format!("Selected {} occurrences — type to replace them all", n));
    }

    fn set_selection(&mut self, i: usize, range: (usize, usize)) {
        let id = egui::Id::new(format!("rea_editor_{i}"));
        let mut st = egui::widgets::text_edit::TextEditState::load(&self.ctx, id).unwrap_or_default();
        st.cursor.set_char_range(Some(egui::text::CCursorRange::two(
            egui::text::CCursor::new(range.0),
            egui::text::CCursor::new(range.1),
        )));
        st.store(&self.ctx, id);
        self.ctx.request_repaint();
    }

    /// Apply the typed text to every multi region (consume-before-Textmodel).
    pub fn multi_apply_typed(&mut self, typed: &str) {
        let Some(m) = self.multi.clone() else { return };
        let i = m.doc;
        if i >= self.docs.len() {
            self.multi = None;
            return;
        }
        let text = self.docs[i].text.clone();
        let new_text = crate::editops::replace_all_ranges(&text, &m.regions, typed);
        let tlen = typed.chars().count();
        // recompute the regions for the NEXT keystroke. When the replacement
        // length differs from the region width, every later region SHIFTS —
        // the old code kept raw starts, so text got inserted at wrong spots.
        let mut sorted = m.regions.clone();
        sorted.sort();
        let mut regions: Vec<(usize, usize)> = Vec::with_capacity(sorted.len());
        let mut delta: isize = 0;
        for r in &sorted {
            let s = (r.0 as isize + delta).max(0) as usize;
            regions.push((s, s + tlen));
            delta += tlen as isize - (r.1.saturating_sub(r.0)) as isize;
        }
        let last_start = regions.last().map(|r| r.0).unwrap_or(0);
        let new_caret = last_start + tlen;
        self.docs[i].text = new_text;
        let txt_c = self.docs[i].text.clone();
        self.docs[i].undo.force(&txt_c, Some(new_caret));
        self.docs[i].dirty = true;
        self.docs[i].last_edit = Some(Instant::now());
        self.word_cache_sig = (usize::MAX, 0);
        let n_regions = regions.len();
        self.multi = Some(MultiEdit { doc: i, regions, caret_region: n_regions - 1 });
        self.set_selection(i, (new_caret, new_caret));
        self.lsp_notify_change(i);
        self.ctx.request_repaint();
    }

    /// Backspace with multi regions active. Deletes the whole region when it
    /// is a selection, else the char before it. Every earlier deletion shifts
    /// later regions left — the old code didn't compensate, so carets drifted
    /// and characters were eaten (#4).
    pub fn multi_backspace(&mut self) {
        let Some(m) = self.multi.clone() else { return };
        let i = m.doc;
        if i >= self.docs.len() {
            self.multi = None;
            return;
        }
        let text = self.docs[i].text.clone();
        let chars: Vec<char> = text.chars().collect();
        // deletion ranges: whole region if selection, else one char before start
        let mut ranges: Vec<(usize, usize)> = Vec::new();
        for (s, e) in m.regions.iter() {
            if e > s {
                ranges.push((*s, *e));
            } else if *s > 0 {
                ranges.push((*s - 1, *s));
            }
        }
        if ranges.is_empty() {
            return;
        }
        let mut new_text = chars;
        ranges.sort_by(|a, b| b.0.cmp(&a.0));
        for &(s, e) in &ranges {
            let (s, e) = (s.min(new_text.len()), e.min(new_text.len()).max(s));
            new_text.splice(s..e, std::iter::empty());
        }
        let out: String = new_text.into_iter().collect();
        // recompute region positions ascending with shift compensation
        let mut sorted = m.regions.clone();
        sorted.sort();
        let mut new_regions: Vec<(usize, usize)> = Vec::new();
        let mut shift: isize = 0;
        let mut first_caret: usize = usize::MAX;
        for (s, e) in sorted {
            if e > s {
                let ns = (s as isize - shift).max(0) as usize;
                new_regions.push((ns, ns));
                shift += (e - s) as isize;
                if first_caret == usize::MAX {
                    first_caret = ns;
                }
            } else if s > 0 {
                let ns = (s as isize - shift - 1).max(0) as usize;
                new_regions.push((ns, ns));
                shift += 1;
                if first_caret == usize::MAX {
                    first_caret = ns;
                }
            } else {
                new_regions.push((0, 0));
                if first_caret == usize::MAX {
                    first_caret = 0;
                }
            }
        }
        self.docs[i].text = out;
        let txt_c = self.docs[i].text.clone();
        self.docs[i].undo.force(&txt_c, Some(first_caret));
        self.docs[i].dirty = true;
        self.docs[i].last_edit = Some(Instant::now());
        let new_regions = {
            let mut v = new_regions;
            v.sort();
            v.dedup();
            v
        };
        self.multi = Some(MultiEdit { doc: i, regions: new_regions, caret_region: 0 });
        self.set_selection(i, (first_caret, first_caret));
        self.lsp_notify_change(i);
        self.ctx.request_repaint();
    }

    /// Cancel multi-cursor state (click, navigation, doc switch…).
    pub fn cancel_multi(&mut self) {
        if self.multi.take().is_some() {
            self.set_status("Multi-cursor off");
        }
    }

    /// Alt+Click — add a secondary cursor at the clicked position (#6).
    pub fn alt_click_add_cursor(&mut self, i: usize, at: usize) {
        if i >= self.docs.len() {
            return;
        }
        let len = self.docs[i].text.chars().count();
        let n = at.min(len);
        let cur = self.cursor_char.unwrap_or(n).min(len);
        if let Some(m) = &mut self.multi {
            if m.doc == i {
                if !m.regions.contains(&(n, n)) {
                    m.regions.push((n, n));
                    m.regions.sort();
                }
                m.caret_region = m.regions.iter().position(|r| *r == (n, n)).unwrap_or(0);
                self.set_selection(i, (n, n));
                return;
            }
        }
        let mut regions: Vec<(usize, usize)> = vec![(cur, cur), (n, n)];
        regions.sort();
        regions.dedup();
        self.multi = Some(MultiEdit { doc: i, regions, caret_region: 0 });
        self.set_selection(i, (n, n));
        self.set_status("Multi-cursor: Alt+Click adds cursors, Esc exits");
    }

    /// Wrap the current selection with a pair char (typed via consume).
    pub fn wrap_selection_with(&mut self, open: char) {
        let Some(i) = self.active else { return };
        let Some((s, e)) = self.sel_range else { return };
        if s == e {
            return;
        }
        let text = self.docs[i].text.clone();
        if let Some((new_text, ns, ne)) = crate::editops::wrap_selection_direct(&text, (s, e), open) {
            self.docs[i].text = new_text;
            let txt_c = self.docs[i].text.clone();
        self.docs[i].undo.force(&txt_c, Some(ne));
            self.docs[i].dirty = true;
            self.docs[i].last_edit = Some(Instant::now());
            self.set_selection(i, (ns, ne));
            self.lsp_notify_change(i);
        }
    }

    /// Post-edit fixups after TextEdit processed a keystroke: auto-pair,
    /// skip-over and auto-indent. Called from editor.rs with the caret.
    pub fn post_edit_fixup(&mut self, i: usize, typed: Option<char>, entered: bool) {
        let cc = match self.cursor_char {
            Some(c) => c,
            None => return,
        };
        let text = self.docs[i].text.clone();
        // 1) auto-pair / skip-over
        if let Some(tch) = typed {
            if self.settings.auto_pair {
                if let Some((nt, nc)) = crate::editops::skip_over(&text, cc, tch) {
                    self.apply_fixup(i, nt, nc);
                    return;
                }
                if let Some((nt, nc)) = crate::editops::auto_pair(&text, cc, tch) {
                    self.apply_fixup(i, nt, nc);
                    return;
                }
            }
        }
        // 2) auto-indent after Enter
        if entered && self.settings.auto_indent {
            if let Some((nt, nc)) = crate::editops::auto_indent(&text, cc, self.settings.tab_size) {
                self.apply_fixup(i, nt, nc);
            }
        }
    }

    fn apply_fixup(&mut self, i: usize, new_text: String, caret: usize) {
        self.docs[i].text = new_text;
        // record this fixup as part of the same undo burst
        let txt_c = self.docs[i].text.clone();
        self.docs[i].undo.after_edit(&txt_c, Some(caret));
        self.docs[i].dirty = true;
        self.docs[i].last_edit = Some(Instant::now());
        self.cursor_char = Some(caret);
        let id = egui::Id::new(format!("rea_editor_{i}"));
        let mut st = egui::widgets::text_edit::TextEditState::load(&self.ctx, id).unwrap_or_default();
        st.cursor
            .set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(caret))));
        st.store(&self.ctx, id);
        self.lsp_notify_change(i);
        self.ctx.request_repaint();
    }

    // ================================================================ v1.5: line editing ops (Ctrl+/ Ctrl+Shift+K Alt+Up/Down)

    /// Toggle `//`/`#`/`--` comments on the cursor line or the whole selection
    /// (Ctrl+/). The toggle_line_comment helper existed but was never wired
    /// to any shortcut — dead code (#9). Block languages (html/css/md) get
    /// wrap-around comments.
    pub fn toggle_comment(&mut self) {
        let Some(i) = self.active else { return };
        let Some((ln, _)) = self.cursor else { return };
        let ext = self.docs[i].ext();
        let text = self.docs[i].text.clone();
        let (start_line, end_line) = match self.sel_range {
            Some((s, e)) if s != e => {
                let total = text.chars().count();
                let (l1, _) = crate::util::char_to_line_col(&text, s.min(e));
                let hi = e.max(s).saturating_sub(1).min(total.saturating_sub(1));
                let (l2, _) = crate::util::char_to_line_col(&text, hi);
                (l1, l2)
            }
            _ => (ln, ln),
        };
        // block-comment languages: wrap / unwrap instead of line markers
        let block: Option<(&str, &str)> = match ext.as_str() {
            "html" | "htm" | "xml" | "svg" | "vue" | "md" | "erb" => Some(("<!-- ", " -->")),
            "css" => Some(("/* ", " */")),
            _ => None,
        };
        let mut lines: Vec<String> = text.split('\n').map(String::from).collect();
        if end_line >= lines.len() {
            return;
        }
        let mut changed = false;
        match block {
            Some((open, close)) => {
                let joined = (start_line..=end_line)
                    .map(|l| lines[l].trim().to_string())
                    .collect::<Vec<_>>()
                    .join("\n");
                if joined.starts_with(open.trim()) && joined.contains(close.trim()) {
                    // unwrap first open + last close in the block
                    for l in start_line..=end_line {
                        if let Some(p) = lines[l].find(open.trim()) {
                            let mut s = lines[l].clone();
                            s.replace_range(p..p + open.trim().len(), "");
                            if let Some(q) = s.rfind(close.trim()) {
                                s.replace_range(q..q + close.trim().len(), "");
                            }
                            lines[l] = s;
                            changed = true;
                            break;
                        }
                    }
                    for l in (start_line..=end_line).rev() {
                        if let Some(q) = lines[l].rfind(close.trim()) {
                            let mut s = lines[l].clone();
                            s.replace_range(q..q + close.trim().len(), "");
                            lines[l] = s;
                            changed = true;
                            break;
                        }
                    }
                } else {
                    let (first, last) = (start_line, end_line);
                    for l in first..=last {
                        if !lines[l].trim().is_empty() {
                            let indent_len = lines[l].len() - lines[l].trim_start().len();
                            let ind = lines[l][..indent_len].to_string();
                            let rest = lines[l][indent_len..].to_string();
                            lines[l] = format!("{ind}{open}{rest}{close}");
                            changed = true;
                        }
                    }
                }
            }
            None => {
                for l in start_line..=end_line {
                    if let Some(nl) = crate::editops::toggle_line_comment(&lines[l], &ext) {
                        lines[l] = nl;
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            return;
        }
        let new_text = lines.join("\n");
        self.docs[i].text = new_text;
        self.docs[i].dirty = true;
        self.docs[i].last_edit = Some(Instant::now());
        let txt_c = self.docs[i].text.clone();
        self.docs[i].undo.force(&txt_c, self.cursor_char);
        self.lsp_notify_change(i);
        self.ctx.request_repaint();
    }

    /// Delete the cursor line or the whole selected block (Ctrl+Shift+K, #9).
    pub fn delete_line(&mut self) {
        let Some(i) = self.active else { return };
        let Some((ln, _)) = self.cursor else { return };
        let text = self.docs[i].text.clone();
        let (start_line, end_line) = match self.sel_range {
            Some((s, e)) if s != e => {
                let total = text.chars().count();
                let (l1, _) = crate::util::char_to_line_col(&text, s.min(e));
                let hi = e.max(s).saturating_sub(1).min(total.saturating_sub(1));
                let (l2, _) = crate::util::char_to_line_col(&text, hi);
                (l1, l2)
            }
            _ => (ln, ln),
        };
        let mut lines: Vec<String> = text.split('\n').map(String::from).collect();
        if start_line >= lines.len() {
            return;
        }
        let end = (end_line + 1).min(lines.len());
        let new_caret_char = crate::util::line_col_to_char(&text, start_line, 0);
        lines.drain(start_line..end);
        let new_text = lines.join("\n");
        let caret = new_caret_char.min(new_text.chars().count());
        self.docs[i].text = new_text;
        self.docs[i].dirty = true;
        self.docs[i].last_edit = Some(Instant::now());
        let txt_c = self.docs[i].text.clone();
        self.docs[i].undo.force(&txt_c, Some(caret));
        self.cursor_char = Some(caret);
        let id = egui::Id::new(format!("rea_editor_{i}"));
        let mut st = egui::widgets::text_edit::TextEditState::load(&self.ctx, id).unwrap_or_default();
        st.cursor
            .set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(caret))));
        st.store(&self.ctx, id);
        self.lsp_notify_change(i);
        self.ctx.request_repaint();
    }

    /// Move the cursor line (or selection block) up/down (Alt+Up / Alt+Down, #9).
    pub fn move_line(&mut self, up: bool) {
        let Some(i) = self.active else { return };
        let Some((ln, col)) = self.cursor else { return };
        let text = self.docs[i].text.clone();
        let (start_line, end_line) = match self.sel_range {
            Some((s, e)) if s != e => {
                let total = text.chars().count();
                let (l1, _) = crate::util::char_to_line_col(&text, s.min(e));
                let hi = e.max(s).saturating_sub(1).min(total.saturating_sub(1));
                let (l2, _) = crate::util::char_to_line_col(&text, hi);
                (l1, l2)
            }
            _ => (ln, ln),
        };
        let mut lines: Vec<String> = text.split('\n').map(String::from).collect();
        if end_line >= lines.len() {
            return;
        }
        if up {
            if start_line == 0 {
                return;
            }
            // block moves above the previous line
            let block: Vec<String> = (start_line..=end_line).map(|l| lines[l].clone()).collect();
            let mut out: Vec<String> = Vec::with_capacity(lines.len());
            out.extend_from_slice(&lines[..start_line - 1]);
            out.extend(block);
            out.push(lines[start_line - 1].clone());
            out.extend_from_slice(&lines[end_line + 1..]);
            lines = out;
        } else {
            if end_line + 1 >= lines.len() {
                return;
            }
            // block moves below the next line
            let block: Vec<String> = (start_line..=end_line).map(|l| lines[l].clone()).collect();
            let mut out: Vec<String> = Vec::with_capacity(lines.len());
            out.extend_from_slice(&lines[..start_line]);
            out.push(lines[end_line + 1].clone());
            out.extend(block);
            out.extend_from_slice(&lines[end_line + 2..]);
            lines = out;
        }
        let new_text = lines.join("\n");
        // keep the caret on the same logical line (moved ±1)
        let target_line = if up { start_line - 1 } else { start_line + 1 };
        let caret = crate::util::line_col_to_char(&new_text, target_line, col).min(new_text.chars().count());
        self.docs[i].text = new_text;
        self.docs[i].dirty = true;
        self.docs[i].last_edit = Some(Instant::now());
        let txt_c = self.docs[i].text.clone();
        self.docs[i].undo.force(&txt_c, Some(caret));
        self.cursor_char = Some(caret);
        let id = egui::Id::new(format!("rea_editor_{i}"));
        let mut st = egui::widgets::text_edit::TextEditState::load(&self.ctx, id).unwrap_or_default();
        st.cursor
            .set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(caret))));
        st.store(&self.ctx, id);
        self.lsp_notify_change(i);
        self.ctx.request_repaint();
    }

    /// Ctrl+G — go-to-line palette (#9).
    pub fn open_goto_line(&mut self) {
        let Some(i) = self.active else {
            self.set_status("Open a file first");
            return;
        };
        let total = 1 + self.docs[i].text.matches('\n').count();
        let mut pal = Palette::new(PaletteMode::GotoLine);
        pal.items = (0..total)
            .map(|l| PalItem {
                id: format!("goto_line:{l}"),
                label: format!("Ln {}", l + 1),
                hint: String::new(),
            })
            .collect();
        pal.refilter();
        self.palette = Some(pal);
        self.find.focus_req = false;
        self.suggest = None;
    }

    // ================================================================ v1.5: explorer file operations (#13)

    pub fn new_file_in(&mut self, dir: PathBuf) {
        self.input_dialog = Some(InputDialog {
            title: "New File".into(),
            label: format!("File name (inside {}):", dir.display()),
            value: String::new(),
            action: format!("newfile:{}", dir.display()),
        });
    }

    pub fn new_folder_in(&mut self, dir: PathBuf) {
        self.input_dialog = Some(InputDialog {
            title: "New Folder".into(),
            label: format!("Folder name (inside {}):", dir.display()),
            value: String::new(),
            action: format!("newfolder:{}", dir.display()),
        });
    }

    pub fn rename_path(&mut self, p: PathBuf) {
        let cur = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        self.input_dialog = Some(InputDialog {
            title: "Rename".into(),
            label: format!("New name for {}:", p.display()),
            value: cur,
            action: format!("rename:{}", p.display()),
        });
    }

    pub fn delete_path(&mut self, p: PathBuf) {
        let name = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let kind = if p.is_dir() { "folder" } else { "file" };
        self.input_dialog = Some(InputDialog {
            title: format!("Delete {kind}"),
            label: format!("Permanently delete '{}'?\nType the name to confirm:", name),
            value: String::new(),
            action: format!("delete:{}", p.display()),
        });
    }

    /// Confirm handler for InputDialog actions.
    pub fn confirm_input_dialog(&mut self) {
        let Some(dlg) = self.input_dialog.clone() else { return };
        self.input_dialog = None;
        let value = dlg.value.trim().to_string();
        if let Some(act) = dlg.action.strip_prefix("bp:") {
            // bp:<path>|<line>
            if let Some((ps, ls)) = act.rsplit_once('|') {
                let path = PathBuf::from(ps);
                if let Ok(line) = ls.parse::<usize>() {
                    let cond = if value.is_empty() { None } else { Some(value) };
                    let entry = self.breakpoints.entry(path.clone()).or_default();
                    if let Some(bp) = entry.iter_mut().find(|b| b.line == line) {
                        bp.condition = cond;
                    } else {
                        entry.push(dap::Bp { line, condition: cond });
                        entry.sort_by_key(|b| b.line);
                    }
                    self.save_breakpoints();
                    self.push_breakpoints_to_session(&path);
                    self.set_status("Breakpoint condition updated");
                }
            }
            return;
        }
        if dlg.action.starts_with("delete:") {
            // guard: the typed name must match the target's file name
            let path = PathBuf::from(dlg.action.trim_start_matches("delete:"));
            let name = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            if value != name {
                self.set_status("Deletion cancelled — name did not match");
                return;
            }
            let ok = if path.is_dir() {
                std::fs::remove_dir_all(&path).is_ok()
            } else {
                std::fs::remove_file(&path).is_ok()
            };
            if ok {
                // close any open tab of that file
                let closes: Vec<usize> = self
                    .docs
                    .iter()
                    .enumerate()
                    .filter(|(_, d)| d.path.as_ref().map(|p| p == &path || p.starts_with(&path)).unwrap_or(false))
                    .map(|(i, _)| i)
                    .collect();
                for i in closes.into_iter().rev() {
                    self.docs[i].dirty = false; // the file is gone; don't block
                    self.close_doc(i);
                }
                self.breakpoints.remove(&path);
                self.diags.remove(&path);
                self.save_breakpoints();
                self.invalidate_dir(path.parent().unwrap_or(Path::new(".")));
                self.set_status(format!("Deleted {}", path.display()));
            } else {
                self.set_status(format!("Could not delete {}", path.display()));
            }
            return;
        }
        let Some((kind, rest)) = dlg.action.split_once(':') else { return };
        if value.is_empty() {
            return;
        }
        match kind {
            "newfile" => {
                let dir = PathBuf::from(rest);
                let target = dir.join(&value);
                if target.exists() {
                    self.set_status("File already exists");
                    return;
                }
                match std::fs::write(&target, "") {
                    Ok(_) => {
                        self.invalidate_dir(&dir);
                        self.open_path(&target);
                    }
                    Err(e) => self.set_status(format!("Create failed: {e}")),
                }
            }
            "newfolder" => {
                let dir = PathBuf::from(rest);
                let target = dir.join(&value);
                match std::fs::create_dir_all(&target) {
                    Ok(_) => {
                        self.invalidate_dir(&dir);
                        self.set_status(format!("Folder created: {}", target.display()));
                    }
                    Err(e) => self.set_status(format!("Create failed: {e}")),
                }
            }
            "rename" => {
                let from = PathBuf::from(rest);
                if value == from.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default() {
                    return;
                }
                let to = from.parent().unwrap_or(Path::new(".")).join(&value);
                match std::fs::rename(&from, &to) {
                    Ok(_) => {
                        // update open docs pointing at the old path
                        for d in self.docs.iter_mut() {
                            if d.path.as_ref() == Some(&from) {
                                d.path = Some(to.clone());
                                d.name = value.clone();
                            }
                        }
                        if let Some(bps) = self.breakpoints.remove(&from) {
                            self.breakpoints.insert(to.clone(), bps);
                            self.save_breakpoints();
                        }
                        self.invalidate_dir(from.parent().unwrap_or(Path::new(".")));
                        self.set_status(format!("Renamed to {}", to.display()));
                    }
                    Err(e) => self.set_status(format!("Rename failed: {e}")),
                }
            }
            _ => {}
        }
    }

    // ---------------- file watcher ----------------

    fn check_disk_changes(&mut self) {
        if self.last_disk_check.elapsed() < Duration::from_millis(700) {
            return;
        }
        self.last_disk_check = Instant::now();
        for i in 0..self.docs.len() {
            let Some(p) = self.docs[i].path.clone() else { continue };
            let Ok(m) = std::fs::metadata(&p) else { continue };
            let mtime = m.modified().ok();
            if mtime == self.docs[i].disk_mtime && m.len() == self.docs[i].disk_len {
                continue;
            }
            // file changed on disk
            if self.docs[i].dirty {
                if self.docs[i].conflict.is_none() {
                    if let Ok(bytes) = std::fs::read(&p) {
                        let (disk, bom, crlf) = crate::util::decode_text(&bytes);
                        if !disk.contains('\0') {
                            self.docs[i].conflict = Some(disk);
                            // keep the disk-side encoding for "keep my version" saves
                            self.docs[i].bom = bom;
                            self.docs[i].crlf = crlf;
                            self.set_status(format!("{} changed on disk", self.docs[i].name));
                        }
                    }
                }
            } else {
                // reload silently
                match std::fs::read(&p) {
                    Ok(bytes) => {
                        let (disk, bom, crlf) = crate::util::decode_text(&bytes);
                        if disk.contains('\0') {
                            continue;
                        }
                        let cc = self.cursor_char.unwrap_or(0);
                        self.docs[i].text = disk;
                        self.docs[i].bom = bom;
                        self.docs[i].crlf = crlf;
                        self.docs[i].disk_mtime = mtime;
                        self.docs[i].disk_len = m.len();
                        let txt_c = self.docs[i].text.clone();
                        let ccl = cc.min(self.docs[i].text.chars().count());
                        self.docs[i].undo = UndoStack::with_initial(&txt_c);
                        self.docs[i].undo.after_edit(&txt_c, Some(ccl));
                        self.word_cache_sig = (usize::MAX, 0);
                        self.set_status(format!("{} reloaded from disk", self.docs[i].name));
                        self.find.sig = None;
                    }
                    Err(_) => {}
                }
            }
        }
    }

    pub fn doc_reload_from_disk(&mut self) {
        let Some(i) = self.active else { return };
        if let Some(disk) = self.docs[i].conflict.clone() {
            let cc = self.cursor_char.unwrap_or(0).min(disk.chars().count());
            self.docs[i].text = disk;
            self.docs[i].conflict = None;
            self.docs[i].dirty = false;
            self.docs[i].recovered = None;
            if let Some(p) = &self.docs[i].path.clone() {
                if let Ok(bytes) = std::fs::read(p) {
                    let (_, bom, crlf) = crate::util::decode_text(&bytes);
                    self.docs[i].bom = bom;
                    self.docs[i].crlf = crlf;
                }
                if let Ok(m) = std::fs::metadata(p) {
                    self.docs[i].disk_mtime = m.modified().ok();
                    self.docs[i].disk_len = m.len();
                }
            }
            let txt_c = self.docs[i].text.clone();
            self.docs[i].undo = UndoStack::with_initial(&txt_c);
            self.docs[i].undo.after_edit(&txt_c, Some(cc));
            self.find.sig = None;
            self.word_cache_sig = (usize::MAX, 0);
            self.set_status("Reloaded from disk");
        }
    }

    pub fn doc_keep_mine(&mut self) {
        let conflict = self.active.and_then(|i| self.docs[i].conflict.clone());
        if conflict.is_some() {
            self.save();
        }
    }

    // ---------------- crash recovery ----------------

    pub fn restore_recovered(&mut self) {
        let Some(i) = self.active else { return };
        if let Some(rec) = self.docs[i].recovered.clone() {
            self.docs[i].text = rec;
            self.docs[i].dirty = true;
            self.docs[i].recovered = None;
            self.docs[i].last_edit = Some(Instant::now());
            let txt_c = self.docs[i].text.clone();
            self.docs[i].undo.after_edit(&txt_c, None);
            self.find.sig = None;
            self.word_cache_sig = (usize::MAX, 0);
            self.set_status("Unsaved changes restored");
        }
    }

    pub fn discard_recovered(&mut self) {
        if let Some(i) = self.active {
            self.docs[i].recovered = None;
        }
    }

    // ---------------- autosave ----------------

    fn autosave_tick(&mut self) {
        if !self.settings.autosave {
            return;
        }
        let due: Vec<usize> = self
            .docs
            .iter()
            .enumerate()
            .filter(|(_, d)| {
                d.dirty
                    && d.path.is_some()
                    && d.conflict.is_none()
                    && d.last_edit.map(|t| t.elapsed() > Duration::from_millis(1500)).unwrap_or(false)
            })
            .map(|(i, _)| i)
            .collect();
        for i in due {
            if let Some(p) = self.docs[i].path.clone() {
                let text = self.docs[i].save_text();
                if std::fs::write(&p, &text).is_ok() {
                    self.docs[i].dirty = false;
                    if let Ok(m) = std::fs::metadata(&p) {
                        self.docs[i].disk_mtime = m.modified().ok();
                        self.docs[i].disk_len = m.len();
                    }
                    let bdir = self.cfg_dir.join("backups");
                    let bkey = self.docs[i].backup_key();
                    self.docs[i].undo.persist(&bdir, &bkey);
                    self.lsp_notify_save(i);
                }
            }
        }
    }

    // ---------------- drag & drop ----------------

    fn handle_dropped(&mut self, ctx: &egui::Context) {
        let dropped: Vec<egui::DroppedFile> =
            ctx.input(|i| i.raw.dropped_files.clone());
        for f in dropped {
            if let Some(p) = f.path {
                if p.is_dir() {
                    self.set_workspace(p);
                } else if p.is_file() {
                    self.open_path(&p);
                }
            }
        }
    }

    // ================================================================ v1.4: LSP

    pub fn lsp_sync_open(&mut self) {
        let Some(i) = self.active else { return };
        let (ext, path, text, lang) = {
            let d = &self.docs[i];
            (d.ext(), d.path.clone(), d.text.clone(), crate::highlight::Highlighter::lang_name(&d.ext()).to_lowercase())
        };
        let Some(p) = path else { return };
        let Some(cmd) = self.settings.lsp.get(&ext).cloned() else { return };
        let Some(root) = self.workspace.clone() else { return };
        if let Some(srv) = self.lsp.server_for(&ext, &cmd, Some(&root)) {
            crate::lsp::set_doc_text(&p, Some(&text));
            crate::lsp::did_open(&srv, &p, &text, &lang);
        }
    }

    pub fn lsp_notify_change(&mut self, i: usize) {
        let Some(p) = self.docs[i].path.clone() else { return };
        let ext = self.docs[i].ext();
        let Some(cmd) = self.settings.lsp.get(&ext).cloned() else { return };
        let Some(srv) = self.lsp.servers.get(&ext).cloned() else { return };
        self.lsp_version_counter += 1;
        let v = self.lsp_version_counter;
        self.docs[i].lsp_version = v;
        let text = self.docs[i].text.clone();
        crate::lsp::set_doc_text(&p, Some(&text));
        crate::lsp::did_change(&srv, &p, &text, v);
        let _ = cmd;
    }

    /// didSave on every successful write (#7).
    pub fn lsp_notify_save(&mut self, i: usize) {
        let Some(p) = self.docs[i].path.clone() else { return };
        let ext = self.docs[i].ext();
        let Some(srv) = self.lsp.servers.get(&ext).cloned() else { return };
        let text = self.docs[i].text.clone();
        crate::lsp::set_doc_text(&p, Some(&text));
        crate::lsp::did_save(&srv, &p, &text);
    }

    /// Drain LSP messages + request completions for the suggest popup.
    pub fn lsp_poll(&mut self) {
        if self.lsp.servers.is_empty() {
            return;
        }
        let root = self.workspace.clone();
        let msgs = self.lsp.drain(root.as_deref());
        for m in msgs {
            crate::lsp::handle_message(&mut self.lsp, m);
        }
        // pull diagnostics for open docs into self.diags
        for d in &self.docs {
            if let Some(p) = &d.path {
                if let Some(list) = self.lsp.diags.get(p) {
                    let list = list.clone();
                    self.diags.insert(p.clone(), list);
                }
            }
        }
    }

    /// Ask the LSP (if any) for completions; results merge into the popup async.
    pub fn lsp_request_completion(&mut self) {
        let Some(i) = self.active else { return };
        let Some(cc) = self.cursor_char else { return };
        let (ext, p, text) = {
            let d = &self.docs[i];
            (d.ext(), d.path.clone(), d.text.clone())
        };
        let (Some(p), Some(cmd)) = (p, self.settings.lsp.get(&ext).cloned()) else { return };
        let Some(srv) = self.lsp.servers.get(&ext).cloned() else { return };
        let id = crate::lsp::request_completion(&srv, &p, &text, cc);
        if id > 0 {
            // request-level clear: old responses must never pollute a new popup (#8)
            self.lsp.completions.clear();
            self.lsp.completions.insert(id, Vec::new());
        }
    }

    pub fn lsp_goto_definition(&mut self) {
        let Some(i) = self.active else { return };
        let Some(cc) = self.cursor_char else { return };
        let (ext, p, text) = {
            let d = &self.docs[i];
            (d.ext(), d.path.clone(), d.text.clone())
        };
        let (Some(p), Some(cmd)) = (p, self.settings.lsp.get(&ext).cloned()) else {
            self.set_status("No LSP configured for this file type (Settings → Language Servers)");
            return;
        };
        let Some(srv) = self.lsp.servers.get(&ext).cloned() else {
            self.set_status("LSP not running — open the file with the server configured");
            return;
        };
        let id = crate::lsp::request_definition(&srv, &p, &text, cc);
        if id > 0 {
            self.lsp.definitions.insert(id, Vec::new());
            self.set_status("Go to definition…");
        }
    }

    /// LSP completion results merged into the active suggest popup.
    pub fn lsp_merge_completions(&mut self) {
        let Some(sg) = self.suggest.clone() else { return };
        let mut extra: Vec<String> = Vec::new();
        // consume each response ONCE — previously the map was never cleared so
        // stale completions from old requests were merged into every popup (#8)
        let ids: Vec<u64> = self.lsp.completions.keys().copied().collect();
        for id in ids {
            if let Some(items) = self.lsp.completions.remove(&id) {
                for it in items {
                    if !it.label.is_empty() && !sg.items.contains(&it.label) {
                        extra.push(it.label.clone());
                    }
                }
            }
        }
        if !extra.is_empty() {
            if let Some(s2) = self.suggest.as_mut() {
                s2.items.extend(extra);
                // cap the list
                s2.items.truncate(60);
            }
        }
        // handle go-to-definition responses
        let mut open: Option<(PathBuf, usize)> = None;
        let ids: Vec<u64> = self.lsp.definitions.keys().copied().collect();
        for id in ids {
            if let Some(locs) = self.lsp.definitions.remove(&id) {
                if let Some((file, line, col)) = locs.first() {
                    let p = PathBuf::from(file);
                    if p.exists() {
                        open = Some((p, *line as usize));
                        let _ = col;
                    }
                }
            }
        }
        if let Some((p, line)) = open {
            self.open_path(&p);
            self.pending_scroll = Some(line);
        }
    }

    // ================================================================ v1.4: debugger

    pub fn debug_start(&mut self) {
        if self.debug.is_some() {
            self.set_status("Debugger already running");
            return;
        }
        let Some(ws) = self.workspace.clone() else {
            self.set_status("Open a folder with a launch.json to debug");
            return;
        };
        let cfgs = match dap::read_launch_json(&ws) {
            Ok(c) => c,
            Err(e) => {
                self.set_status(e);
                return;
            }
        };
        // v1.7.3 trust gate: launch.json can start ANY program — a cloned
        // repo must never run silently. Ask once per folder per session.
        let key = ws
            .canonicalize()
            .unwrap_or_else(|_| ws.clone())
            .display()
            .to_string()
            .to_lowercase();
        if !self.dap_trusted.contains(&key) {
            self.dap_trust_req = Some(dap::TrustReq {
                key,
                root_display: ws.display().to_string(),
                cfgs,
            });
            return;
        }
        self.debug_launch(cfgs);
    }

    /// actually spawn the adapter (after the trust dialog approved it)
    pub fn debug_launch(&mut self, cfgs: Vec<dap::LaunchCfg>) {
        self.launch_cfg = cfgs.first().cloned();
        let Some(cfg) = self.launch_cfg.clone() else { return };
        match dap::DapSession::spawn(&cfg) {
            Ok(s) => {
                self.debug = Some(s);
                self.terminal_visible = true;
                self.set_status(format!("Debugging: {}", cfg.name));
            }
            Err(e) => self.set_status(e),
        }
    }

    pub fn dap_trust_allow(&mut self) {
        if let Some(r) = self.dap_trust_req.take() {
            self.dap_trusted.insert(r.key);
            self.debug_launch(r.cfgs);
        }
    }

    pub fn dap_trust_once(&mut self) {
        if let Some(r) = self.dap_trust_req.take() {
            self.debug_launch(r.cfgs);
        }
    }

    pub fn dap_trust_cancel(&mut self) {
        self.dap_trust_req = None;
        self.set_status("Debug start cancelled");
    }

    pub fn debug_stop(&mut self) {
        if let Some(s) = self.debug.as_mut() {
            s.stop();
        }
        self.debug = None;
        self.set_status("Debugger stopped");
    }

    pub fn debug_continue(&mut self) {
        if let Some(s) = self.debug.as_mut() {
            s.req("continue", json!({"threadId": 1}));
            s.state = dap::DbgState::Running;
            s.frames.clear();
            s.variables.clear();
        }
    }

    pub fn debug_step(&mut self, over: bool) {
        if let Some(s) = self.debug.as_mut() {
            let cmd = if over { "next" } else { "stepIn" };
            s.req(cmd, json!({"threadId": 1}));
            s.state = dap::DbgState::Running;
        }
    }

    pub fn debug_restart(&mut self) {
        self.debug_stop();
        self.debug_start();
    }

    pub fn debug_select_frame(&mut self, idx: usize) {
        let target = self.debug.as_ref().and_then(|s| {
            s.frames.get(idx).and_then(|f| f.file.clone().map(|fp| (fp, f.line.saturating_sub(1))))
        });
        if let Some((fp, line)) = target {
            if let Some(s) = self.debug.as_mut() {
                s.current_frame = idx;
            }
            self.open_path(&fp);
            self.pending_scroll = Some(line);
        }
    }

    pub fn toggle_breakpoint(&mut self, path: PathBuf, line: usize) {
        let entry = self.breakpoints.entry(path.clone()).or_default();
        if let Some(pos) = entry.iter().position(|b| b.line == line) {
            entry.remove(pos);
        } else {
            entry.push(dap::Bp::simple(line));
            entry.sort_by_key(|b| b.line);
        }
        self.save_breakpoints();
        // live update if debugging (DAP lines are 1-BASED, we store 0-based, #5)
        self.push_breakpoints_to_session(&path);
    }

    /// Open the conditional-breakpoint editor for a line (#16).
    pub fn edit_breakpoint_condition(&mut self, path: PathBuf, line: usize) {
        let cur = self
            .breakpoints
            .get(&path)
            .and_then(|l| l.iter().find(|b| b.line == line))
            .and_then(|b| b.condition.clone())
            .unwrap_or_default();
        self.input_dialog = Some(InputDialog {
            title: "Conditional Breakpoint".into(),
            label: format!("Break when this expression is true — line {}\n(empty = always break)", line + 1),
            value: cur,
            action: format!("bp:{}|{}", path.display(), line),
        });
    }

    /// Push breakpoints for one file into a running debug session.
    fn push_breakpoints_to_session(&mut self, path: &Path) {
        let lines = self.breakpoints.get(path).cloned().unwrap_or_default();
        if let Some(s) = self.debug.as_mut() {
            let bps: Vec<serde_json::Value> = lines
                .iter()
                .map(|bp| {
                    let mut j = json!({"line": bp.line + 1}); // 1-based over DAP (#5)
                    if let Some(cond) = &bp.condition {
                        j["condition"] = json!(cond);
                    }
                    j
                })
                .collect();
            s.req(
                "setBreakpoints",
                json!({"source": {"path": path.display().to_string()}, "breakpoints": bps}),
            );
        }
    }

    // ---------------- breakpoint persistence (#16) ----------------

    fn bp_file(&self) -> PathBuf {
        self.cfg_dir.join("breakpoints.json")
    }

    fn save_breakpoints(&self) {
        let clean: std::collections::BTreeMap<String, Vec<dap::Bp>> = self
            .breakpoints
            .iter()
            .filter(|(_, v)| !v.is_empty())
            .map(|(k, v)| (k.display().to_string(), v.clone()))
            .collect();
        if let Ok(j) = serde_json::to_string_pretty(&clean) {
            let _ = std::fs::write(self.bp_file(), j);
        }
    }

    pub fn load_breakpoints(&mut self) {
        if let Ok(s) = std::fs::read_to_string(self.bp_file()) {
            if let Ok(map) = serde_json::from_str::<std::collections::BTreeMap<String, Vec<dap::Bp>>>(&s) {
                self.breakpoints = map
                    .into_iter()
                    .map(|(k, v)| (PathBuf::from(k), v))
                    .collect();
            }
        }
    }

    // ---------------- watch / hover evaluate (#16) ----------------

    pub fn debug_watch(&mut self, expr: &str, frame_id: u64) {
        if let Some(s) = self.debug.as_mut() {
            s.evaluate(expr, "watch", frame_id);
        }
    }

    pub fn debug_remove_watch(&mut self, expr: &str) {
        if let Some(s) = self.debug.as_mut() {
            s.watches.retain(|(e, _)| e != expr);
        }
    }

    /// Hover-eval: evaluate the word under the mouse while paused (#16).
    /// Called from editor.rs; throttled per word + 500ms.
    pub fn debug_hover_eval(&mut self, word: String, pos: egui::Pos2) {
        let paused = self.debug.as_ref().map(|s| s.state == dap::DbgState::Paused).unwrap_or(false);
        if !paused || word.is_empty() {
            self.hover_eval = None;
            return;
        }
        let now = Instant::now();
        let fresh = match &self.hover_eval {
            Some((w, p, t)) => *w == word && p.distance(pos) < 6.0 && t.elapsed().as_millis() < 700,
            None => false,
        };
        if fresh {
            return;
        }
        self.hover_eval = Some((word.clone(), pos, now));
        if let Some(s) = self.debug.as_mut() {
            let frame_id = s.frames.get(s.current_frame).map(|f| f.id).unwrap_or(0);
            s.evaluate(&word, "hover", frame_id);
        }
    }

    pub fn debug_poll(&mut self) {
        let Some(sess_alive) = self.debug.as_ref().map(|s| s.is_alive()) else { return };
        if !sess_alive {
            self.debug = None;
            self.set_status("Debug session ended");
            return;
        }
        let msgs = self.debug.as_mut().unwrap().drain();
        for m in msgs {
            let bps = self.breakpoints.clone();
            if let Some(s) = self.debug.as_mut() {
                if let Some((file, line)) = dap::handle_message(s, m, &bps) {
                    // paused at a location → jump there
                    self.open_path(&file);
                    self.pending_scroll = Some(line.saturating_sub(1));
                }
            }
        }
        if self.debug.is_some() {
            self.ctx.request_repaint_after(Duration::from_millis(150));
        }
    }

    // ================================================================ commands

    // ================================================================ v2.1: tools / diff / todo / stats

    /// Run a text-tools operation on the active document (see texttools::apply).
    /// `msg` is shown on success; Info outcomes (hashes) are copied to the
    /// clipboard automatically.
    pub fn tool_run(&mut self, op: &str, msg: &str) {
        let Some(i) = self.active else {
            self.set_status("Open a file first");
            return;
        };
        let text = self.docs[i].text.clone();
        let range = self
            .sel_range
            .filter(|(s, e)| e > s)
            .map(|(s, e)| (s.min(e), e.max(s)));
        let caret = self.cursor_char.unwrap_or(0);
        match crate::texttools::apply(op, &text, range, caret) {
            Ok(crate::texttools::Outcome::Text(new_text, new_caret)) => {
                self.tool_commit(i, new_text, new_caret, msg);
            }
            Ok(crate::texttools::Outcome::Info(info)) => {
                self.ctx.output_mut(|o| o.copied_text = info.clone());
                self.set_status(format!("{} — copied to clipboard", info));
            }
            Err(e) => self.set_status(e),
        }
    }

    /// Commit a programmatic whole-document edit (single undo step).
    fn tool_commit(&mut self, i: usize, new_text: String, caret: usize, msg: &str) {
        self.docs[i].text = new_text;
        let t = self.docs[i].text.clone();
        self.docs[i].undo.force(&t, Some(caret));
        self.docs[i].dirty = true;
        self.docs[i].last_edit = Some(Instant::now());
        self.word_cache_sig = (usize::MAX, 0);
        self.set_selection(i, (caret, caret));
        self.lsp_notify_change(i);
        self.set_status(msg);
        self.ctx.request_repaint();
    }

    /// EOL switch — the doc keeps LF in memory and restores the flag on save
    /// (same mechanism the loader already uses for CRLF files).
    pub fn eol_set(&mut self, crlf: bool) {
        let Some(i) = self.active else {
            self.set_status("Open a file first");
            return;
        };
        if self.docs[i].crlf == crlf {
            self.set_status(if crlf { "Already CRLF" } else { "Already LF" });
            return;
        }
        self.docs[i].crlf = crlf;
        self.docs[i].dirty = true;
        self.set_status(if crlf {
            "EOL: Windows (CRLF) — applied on save"
        } else {
            "EOL: Unix (LF) — applied on save"
        });
    }

    /// Add/remove the UTF-8 BOM (written on save, like CRLF).
    pub fn bom_toggle(&mut self) {
        let Some(i) = self.active else {
            self.set_status("Open a file first");
            return;
        };
        self.docs[i].bom = !self.docs[i].bom;
        self.docs[i].dirty = true;
        self.set_status(if self.docs[i].bom {
            "UTF-8 BOM added (written on save)"
        } else {
            "UTF-8 BOM removed (written on save)"
        });
    }

    /// Open the Document Statistics window (snapshot of the active doc).
    pub fn doc_stats_open(&mut self) {
        let Some(i) = self.active else {
            self.set_status("Open a file first");
            return;
        };
        let sel = self
            .sel_range
            .filter(|(s, e)| e > s)
            .map(|(s, e)| (s.min(e), e.max(s)));
        let disk_len = self.docs[i]
            .path
            .as_ref()
            .and_then(|p| std::fs::metadata(p).ok())
            .map(|m| m.len())
            .unwrap_or(0);
        self.stats = Some(crate::texttools::DocStats::compute(
            &self.docs[i].text,
            sel,
            disk_len,
            self.docs[i].crlf,
            self.docs[i].bom,
        ));
        self.stats_open = true;
    }

    pub fn run_command(&mut self, id: &str, _ctx: &egui::Context) {
        if let Some(p) = id.strip_prefix("goto_line:") {
            if let Ok(line) = p.parse::<usize>() {
                self.pending_scroll = Some(line); // 0-based line
            }
            return;
        }
        if let Some(dir) = id.strip_prefix("newfile_in:") {
            self.new_file_in(PathBuf::from(dir));
            return;
        }
        if let Some(dir) = id.strip_prefix("newfolder_in:") {
            self.new_folder_in(PathBuf::from(dir));
            return;
        }
        if let Some(p) = id.strip_prefix("open:") {
            let path = PathBuf::from(p);
            if path.exists() {
                self.open_path(&path);
            } else {
                self.recents.retain(|x| x != &path);
                self.set_status("File no longer exists");
            }
            return;
        }
        if let Some(p) = id.strip_prefix("openfolder:") {
            let path = PathBuf::from(p);
            if path.exists() {
                self.set_workspace(path);
            } else {
                self.recent_folders.retain(|x| x.display().to_string() != p);
                self.set_status("Folder no longer exists");
            }
            return;
        }
        match id {
            "new_file" => self.new_file(),
            "open_file" => self.open_file_dialog(),
            "open_folder" => self.open_folder_dialog(),
            "save" => self.save(),
            "save_as" => self.save_as_dialog(),
            "find" => self.open_find(),
            "replace" => {
                self.open_find();
                self.find.show_replace = true;
                self.find.replace_focus_req = true;
            }
            "find_in_files" => {
                self.panel_mode = PanelMode::Search;
                self.show_sidebar = true;
                self.refresh_tree();
            }
            "undo" => self.do_undo(),
            "redo" => self.do_redo(),
            "multi_next" => self.multi_select_next(),
            "multi_all" => self.multi_select_all(),
            "goto_definition" => self.lsp_goto_definition(),
            "save_all" => self.save_all(),
            "toggle_dotfiles" => {
                self.settings.show_dotfiles = !self.settings.show_dotfiles;
                self.save_settings();
                self.refresh_tree();
            }
            "toggle_wrap" => {
                self.settings.word_wrap = !self.settings.word_wrap;
                self.save_settings();
            }
            "toggle_breadcrumbs" => {
                self.settings.breadcrumbs = !self.settings.breadcrumbs;
                self.save_settings();
            }
            "toggle_autosave" => {
                self.settings.autosave = !self.settings.autosave;
                self.save_settings();
            }
            "settings" => {
                self.panel_mode = PanelMode::Settings;
                self.show_sidebar = true;
            }
            "start_debug" => self.debug_start(),
            "stop_debug" => self.debug_stop(),
            "goto_line" => self.open_goto_line(),
            "toggle_comment" => self.toggle_comment(),
            "toggle_breakpoint" => {
                if let (Some(i), Some((ln, _))) = (self.active, self.cursor) {
                    if let Some(p) = self.docs[i].path.clone() {
                        self.toggle_breakpoint(p, ln);
                    }
                }
            }
            "install_vsix" => crate::extensions::install_vsix_dialog(self),
            "toggle_theme" => self.toggle_theme(),
            "toggle_terminal" => self.toggle_terminal(),
            "toggle_explorer" => self.toggle_explorer(),
            // ---- v1.9 ----
            "bookmark_toggle" => self.toggle_bookmark(),
            "bookmark_next" => self.bookmark_nav(true),
            "bookmark_prev" => self.bookmark_nav(false),
            "bookmark_clear" => self.bookmark_clear_doc(),
            "zoom_in" => self.zoom_step(1.15),
            "zoom_out" => self.zoom_step(1.0 / 1.15),
            "zoom_reset" => self.zoom = 1.0,
            // ---- v2.0: macro + column editing ----
            "macro_record" => self.macro_record_toggle(),
            "macro_play" => self.macro_play(),
            "col_select" => self.col_start(),
            "outline" => self.click_panel(PanelMode::Outline),
            "restore_session" => self.restore_last_session(),
            "check_updates" => self.upd_check_now(),
            "extensions" => {
                self.panel_mode = PanelMode::Extensions;
                self.show_sidebar = true;
            }
            "close_all" => self.close_all_docs(),
            "about" => self.about_open = true,
            "github_clone" => self.open_github_dialog(),
            "restart_terminal" => self.restart_terminal(),
            // ---- v1.7 ----
            "new_terminal" => self.spawn_terminal(crate::terminal::ShellKind::PowerShell),
            "new_terminal_cmd" => self.spawn_terminal(crate::terminal::ShellKind::Cmd),
            "git_panel" => {
                self.bottom_tab = BottomTab::Git;
                self.terminal_visible = true;
                self.git_refresh();
            }
            "git_refresh" => self.git_refresh(),
            "git_stage_all" => self.git_stage_all(),
            "format_doc" => crate::formatter::format_active(self),
            "split_editor" => self.toggle_split(),
            "toggle_minimap" => {
                self.settings.minimap = !self.settings.minimap;
                self.save_settings();
            }
            // ---- v1.6 plugins ----
            "open_browser" => self.open_active_in_browser(),
            "live_start" => self.live_start(),
            "live_stop" => self.live_stop(),
            "rest_send" => {
                self.bottom_tab = BottomTab::Rest;
                self.terminal_visible = true;
                self.rest_send_active();
            }
            "rest_panel" => {
                self.bottom_tab = BottomTab::Rest;
                self.terminal_visible = true;
            }
            "recco_show" => self.suggest_force_open(),
            "plugins" => {
                self.panel_mode = PanelMode::Extensions;
                self.show_sidebar = true;
            }
            // ---- v2.1: tools / diff / todo ----
            "diff_files" => crate::diffview::open_dialog(self),
            "diff_next" => {
                if let Some(d) = &mut self.diff {
                    d.nav(true);
                }
            }
            "diff_prev" => {
                if let Some(d) = &mut self.diff {
                    d.nav(false);
                }
            }
            "todo_panel" => {
                self.panel_mode = PanelMode::Todos;
                self.show_sidebar = true;
                crate::todopanel::scan_start(self);
            }
            "doc_stats" => self.doc_stats_open(),
            "tool_sort_asc" => self.tool_run("sort_asc", "Sorted lines A → Z"),
            "tool_sort_desc" => self.tool_run("sort_desc", "Sorted lines Z → A"),
            "tool_dedupe" => self.tool_run("dedupe", "Duplicate lines removed"),
            "tool_rm_empty" => self.tool_run("rm_empty", "Empty lines removed"),
            "tool_reverse" => self.tool_run("reverse", "Line order reversed"),
            "tool_trim_trail" => self.tool_run("trim_trail", "Trailing spaces trimmed"),
            "tool_trim_lead" => self.tool_run("trim_lead", "Leading spaces trimmed"),
            "tool_upper" => self.tool_run("upper", "Uppercased"),
            "tool_lower" => self.tool_run("lower", "Lowercased"),
            "tool_title" => self.tool_run("title", "Title Cased"),
            "tool_invert" => self.tool_run("invert", "Case inverted"),
            "tool_b64_enc" => self.tool_run("b64_enc", "Base64 encoded"),
            "tool_b64_dec" => self.tool_run("b64_dec", "Base64 decoded"),
            "tool_url_enc" => self.tool_run("url_enc", "URL encoded"),
            "tool_url_dec" => self.tool_run("url_dec", "URL decoded"),
            "tool_json_esc" => self.tool_run("json_esc", "JSON-escaped"),
            "tool_json_unesc" => self.tool_run("json_unesc", "JSON-unescaped"),
            "tool_md5" => self.tool_run("md5", "MD5"),
            "tool_sha1" => self.tool_run("sha1", "SHA-1"),
            "tool_sha256" => self.tool_run("sha256", "SHA-256"),
            "tool_timestamp" => self.tool_run("timestamp", "Timestamp inserted"),
            "eol_lf" => self.eol_set(false),
            "eol_crlf" => self.eol_set(true),
            "bom_toggle" => self.bom_toggle(),
            _ => {}
        }
    }

    // ================================================================ shortcuts (typing fix:
    // consume_key with EXACT modifiers, executed BEFORE any widget renders, so
    // plain typing always reaches the focused text field untouched)

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        use egui::Key;

        // --- v2.1: diff-view hunk navigation (only while a compare is open) ---
        if self.diff.is_some() {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::F8)) {
                if let Some(d) = &mut self.diff {
                    d.nav(true);
                }
            }
            if ctx
                .input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, Key::F8))
            {
                if let Some(d) = &mut self.diff {
                    d.nav(false);
                }
            }
        }

        // --- v2.0: macro recording (PASSIVE — never consumes; the keystrokes
        // keep doing their live thing while we copy them). Only captured when
        // the editor is focused, so typing in Find/terminal isn't recorded. ---
        if self.macro_rec.is_some() && self.editor_focused {
            let evs: Vec<crate::macros::MEv> = ctx.input(|i| {
                i.events
                    .iter()
                    .filter_map(|e| match e {
                        egui::Event::Text(t) => Some(crate::macros::MEv::Text(t.clone())),
                        egui::Event::Key { key, pressed: true, modifiers, .. } if !modifiers.any() => {
                            use crate::macros::{MKey, MEv};
                            let mk = match key {
                                Key::Backspace => Some(MKey::Backspace),
                                Key::Delete => Some(MKey::Delete),
                                Key::Enter => Some(MKey::Enter),
                                Key::Tab => Some(MKey::Tab),
                                Key::ArrowLeft => Some(MKey::Left),
                                Key::ArrowRight => Some(MKey::Right),
                                Key::ArrowUp => Some(MKey::Up),
                                Key::ArrowDown => Some(MKey::Down),
                                Key::Home => Some(MKey::Home),
                                Key::End => Some(MKey::End),
                                _ => None,
                            };
                            mk.map(MEv::Key)
                        }
                        _ => None,
                    })
                    .collect()
            });
            if !evs.is_empty() {
                if let Some(rec) = &mut self.macro_rec {
                    let room = 20_000usize.saturating_sub(rec.len());
                    rec.extend(evs.into_iter().take(room));
                    if rec.len() >= 20_000 {
                        self.set_status("Macro too long — recording stopped");
                        self.macro_rec = None;
                    }
                }
            }
        }

        // --- v2.0: column (block) selection — Notepad++ Column Mode ---
        if self.editor_focused {
            // Alt+Shift+Arrows: start + extend
            for (key, dl, dc) in [
                (Key::ArrowDown, 1isize, 0isize),
                (Key::ArrowUp, -1, 0),
                (Key::ArrowRight, 0, 1),
                (Key::ArrowLeft, 0, -1),
            ] {
                if ctx.input_mut(|i| {
                    i.consume_key(egui::Modifiers::ALT | egui::Modifiers::SHIFT, key)
                }) {
                    self.col_extend(dl, dc);
                    return;
                }
            }
            if self.colsel.is_some() {
                let typed: String = ctx.input(|i| {
                    i.events
                        .iter()
                        .filter_map(|e| match e {
                            egui::Event::Text(t) => Some(t.clone()),
                            _ => None,
                        })
                        .collect()
                });
                if !typed.is_empty() {
                    // remove the text events so TextEdit doesn't double-insert
                    ctx.input_mut(|i| {
                        i.events
                            .retain(|e| !matches!(e, egui::Event::Text(_)))
                    });
                    self.col_type(&typed);
                    return;
                }
                if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Backspace)) {
                    self.col_backspace();
                    return;
                }
                if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Delete)) {
                    self.col_delete();
                    return;
                }
                if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape)) {
                    self.col_exit();
                    self.set_status("Column mode off");
                    return;
                }
                // any other cursor-moving key exits column mode silently
                for k in [
                    Key::ArrowUp, Key::ArrowDown, Key::ArrowLeft, Key::ArrowRight,
                    Key::Enter, Key::Home, Key::End, Key::PageUp, Key::PageDown,
                ] {
                    if ctx.input(|i| i.key_pressed(k)) {
                        self.col_exit();
                        break;
                    }
                }
            }
        }

        // --- multi-occurrence editing: intercept typing BEFORE TextEdit ---
        if self.multi.is_some() && self.editor_focused {
            let typed: String = ctx.input(|i| {
                i.events
                    .iter()
                    .filter_map(|e| match e {
                        egui::Event::Text(t) => Some(t.clone()),
                        _ => None,
                    })
                    .collect()
            });
            if !typed.is_empty() {
                // remove the text events so TextEdit doesn't double-insert
                ctx.input_mut(|i| {
                    i.events
                        .retain(|e| !matches!(e, egui::Event::Text(_)))
                });
                self.multi_apply_typed(&typed);
                return;
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Backspace)) {
                self.multi_backspace();
                return;
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape)) {
                self.multi = None;
                self.set_status("Multi-cursor off");
                return;
            }
            for k in [Key::ArrowUp, Key::ArrowDown, Key::ArrowLeft, Key::ArrowRight, Key::Enter, Key::Home, Key::End] {
                if ctx.input(|i| i.key_pressed(k)) {
                    self.multi = None; // let normal navigation happen (#2)
                    break;
                }
            }
        }

        // --- selection wrap: selection + pair char typed → wrap instead of overwrite ---
        if self.editor_focused && self.sel_range.map(|(s, e)| s != e).unwrap_or(false) {
            let pair_typed: Option<char> = ctx.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Text(t) => {
                        let mut cs = t.chars();
                        match (cs.next(), cs.next()) {
                            (Some(c), None) if crate::editops::is_pair_char(c) => Some(c),
                            _ => None,
                        }
                    }
                    _ => None,
                })
            });
            if let Some(c) = pair_typed {
                ctx.input_mut(|i| {
                    i.events
                        .retain(|e| !matches!(e, egui::Event::Text(t) if t.chars().count() == 1))
                });
                self.wrap_selection_with(c);
                return;
            }
        }

        // --- code suggestions FIRST (popup open + editor focused): VS Code-like keys ---
        if self.suggest.is_some() && self.editor_focused {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape)) {
                self.suggest = None;
                return;
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::ArrowDown)) {
                self.move_suggest_sel(1);
                return;
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::ArrowUp)) {
                self.move_suggest_sel(-1);
                return;
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Enter)) {
                self.accept_suggestion(ctx);
                return;
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Tab)) {
                self.accept_suggestion(ctx);
                return;
            }
        }

        // --- undo/redo (consumed BEFORE TextEdit so egui's built-in undo never fights ours) ---
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, Key::Z)) {
            self.do_undo();
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, Key::Z)) {
            self.do_redo();
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, Key::Y)) {
            self.do_redo();
            return;
        }

        // --- v1.9: bookmarks + editor zoom (consumed BEFORE TextEdit) ---
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, Key::F2)) {
            self.toggle_bookmark();
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::F2)) {
            self.bookmark_nav(true);
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, Key::F2)) {
            self.bookmark_nav(false);
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::ALT, Key::F2)) {
            self.bookmark_clear_doc();
            return;
        }
        // --- v2.0: macro record / play ---
        if ctx.input_mut(|i| {
            i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, Key::R)
        }) {
            self.macro_record_toggle();
            return;
        }
        if ctx.input_mut(|i| {
            i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, Key::M)
        }) {
            self.macro_play();
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, Key::Equals)) {
            self.zoom_step(1.15);
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, Key::Minus)) {
            self.zoom_step(1.0 / 1.15);
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, Key::Num0)) {
            self.zoom = 1.0;
            return;
        }
        {
            // Ctrl+wheel zoom over the editor (sign-based, one step per frame)
            let wz = ctx.input(|i| {
                i.events
                    .iter()
                    .filter_map(|e| match e {
                        egui::Event::MouseWheel { delta, modifiers, .. } if modifiers.ctrl => {
                            Some(delta.y)
                        }
                        _ => None,
                    })
                    .sum::<f32>()
            });
            if wz > 0.5 {
                self.zoom_step(1.15);
            } else if wz < -0.5 {
                self.zoom_step(1.0 / 1.15);
            }
        }

        // --- palette keys (when open) ---
        if self.palette.is_some() {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape)) {
                self.palette = None;
                return;
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::ArrowDown)) {
                if let Some(p) = self.palette.as_mut() {
                    p.move_sel(1);
                }
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::ArrowUp)) {
                if let Some(p) = self.palette.as_mut() {
                    p.move_sel(-1);
                }
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Enter)) {
                let id = self.palette.as_ref().and_then(|p| p.current_id());
                self.palette = None;
                if let Some(id) = id {
                    self.run_command(&id, ctx);
                }
                return;
            }
        }

        // --- find keys (Enter/Shift+Enter only when the find input is focused,
        // so the terminal and other fields keep working while find is open) ---
        if self.find.open {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape)) {
                self.close_find();
            }
            let fi_focused = self.find.input_focused;
            if fi_focused {
                if ctx.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, Key::Enter)) {
                    self.find_prev();
                }
                if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Enter)) {
                    self.find_next();
                }
            }
            // Ctrl+H toggles the replace row
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, Key::H)) {
                self.find.show_replace = !self.find.show_replace;
                if self.find.show_replace {
                    self.find.replace_focus_req = true;
                }
            }
        }

        // --- global combos (exact modifier match — typing letters is never eaten) ---
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, Key::Space)) {
            self.suggest_force_open();
            self.lsp_request_completion();
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, Key::G)) {
            self.open_github_dialog();
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, Key::P)) {
            self.open_palette(PaletteMode::Commands);
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, Key::P)) {
            self.open_palette(PaletteMode::Files);
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, Key::O)) {
            self.open_folder_dialog();
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, Key::F)) {
            self.panel_mode = PanelMode::Search;
            self.show_sidebar = true;
            self.refresh_tree();
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, Key::H)) {
            self.open_find();
            self.find.show_replace = true;
            self.find.replace_focus_req = true;
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, Key::S)) {
            self.save_as_dialog();
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, Key::D)) {
            self.multi_select_next();
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, Key::L)) {
            self.multi_select_all();
            return;
        }
        // --- v1.5 line-editing shortcuts (#9) ---
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, Key::Slash)) {
            self.toggle_comment();
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, Key::K)) {
            self.delete_line();
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::ALT, Key::ArrowUp)) {
            self.move_line(true);
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::ALT, Key::ArrowDown)) {
            self.move_line(false);
            return;
        }
        // NOTE: Ctrl+Shift+G (GitHub) is consumed above — plain Ctrl+G goes to line
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, Key::G)) {
            self.open_goto_line();
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::F12)) {
            self.lsp_goto_definition();
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::F5)) {
            self.debug_start();
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, Key::F5)) {
            self.debug_stop();
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::F9)) {
            if let (Some(i), Some((ln, _))) = (self.active, self.cursor) {
                if let Some(p) = self.docs[i].path.clone() {
                    self.toggle_breakpoint(p, ln);
                }
            }
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, Key::Comma)) {
            self.panel_mode = PanelMode::Settings;
            self.show_sidebar = true;
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::ALT, Key::Z)) {
            self.settings.word_wrap = !self.settings.word_wrap;
            self.save_settings();
            return;
        }
        // --- v1.6 plugin shortcuts ---
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::ALT | egui::Modifiers::SHIFT, Key::F)) {
            crate::formatter::format_active(self);
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::ALT, Key::B)) {
            self.open_active_in_browser();
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::ALT, Key::L)) {
            if self.live.running {
                self.live_stop();
            } else {
                self.live_start();
            }
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::ALT, Key::R)) {
            self.bottom_tab = BottomTab::Rest;
            self.terminal_visible = true;
            self.rest_send_active();
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, Key::N)) {
            self.new_file();
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, Key::O)) {
            self.open_file_dialog();
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, Key::S)) {
            self.save();
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, Key::F)) {
            self.palette = None;
            self.open_find();
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, Key::Backtick)) {
            self.toggle_terminal();
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, Key::B)) {
            self.toggle_explorer();
            return;
        }
    }
}

fn cmd(id: &str, label: &str, hint: &str) -> PalItem {
    PalItem {
        id: id.into(),
        label: label.into(),
        hint: hint.into(),
    }
}

fn config_dir() -> PathBuf {
    if let Ok(appdata) = std::env::var("APPDATA") {
        return PathBuf::from(appdata).join("REAStudio");
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".rea-studio");
    }
    PathBuf::from(".rea-studio")
}

// ---------------------------------------------------------------- eframe App impl

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 0) intercept the window close: ask about unsaved changes (#12)
        if !self.exit_allow && ctx.input(|i| i.viewport().close_requested()) {
            let any_dirty = self.docs.iter().any(|d| d.dirty);
            if any_dirty {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.exit_confirm = true;
            }
        }

        // 1) theme animation (pure-Rust lerp, ~320ms smoothstep)
        if let Some((from, to, start)) = self.anim.take() {
            let t = (start.elapsed().as_secs_f32() / 0.32).min(1.0);
            let e = t * t * (3.0 - 2.0 * t);
            let th = theme::lerp(&from, &to, e);
            theme::apply(ctx, &th);
            if t >= 1.0 {
                self.theme_now = to;
            } else {
                self.theme_now = th;
                self.anim = Some((from, to, start));
            }
            // CRITICAL: egui 0.29's set_visuals() does NOT schedule a repaint,
            // so without this pump the lerp freezes after one frame and the
            // theme "jumps" instead of animating. 8ms -> ~120fps transition.
            ctx.request_repaint_after(Duration::from_millis(8));
        }

        // 2) global shortcuts BEFORE widgets (typing fix)
        self.handle_shortcuts(ctx);

        // 2b) font-size application + drag & drop
        self.apply_font_size();
        self.handle_dropped(ctx);

        // 2b-v1.9) update-check poll + outline re-scan (both no-ops when fresh)
        self.upd_poll();
        self.outline_refresh();
        // v2.1: TODO scanner background job pickup (no-op when not scanning)
        crate::todopanel::poll(self);

        // 2c) file watcher + autosave + explorer dir-watch (#14)
        self.check_disk_changes();
        self.check_dir_changes();
        self.autosave_tick();

        // 2d) LSP + debugger polling
        self.lsp_poll();
        self.lsp_merge_completions();
        self.debug_poll();
        // persist undo history for the active doc once a second
        if let Some(i) = self.active {
            let key = self.docs[i].backup_key();
            let bdir = self.cfg_dir.join("backups");
            self.docs[i].undo.persist(&bdir, &key);
        }

        // 3) terminal lifecycle + github clone polling
        if self.terminal_visible {
            self.ensure_terminal();
        }
        // v1.7: git panel background job polling
        self.git_poll();
        // v1.6: REST Client background job polling + live server repaint
        if self.rest.busy {
            self.rest_poll();
            ctx.request_repaint_after(Duration::from_millis(120));
        }
        if self.clone_job.is_some() || self.github.is_some() {
            self.poll_clone();
            if self.clone_job.is_some() {
                ctx.request_repaint_after(Duration::from_millis(120));
            }
        }

        // 4) panels — order matters: status first (outermost bottom), then terminal
        crate::bars::show_status(self, ctx);
        if self.debug.is_some() {
            let t2 = self.theme_now.clone();
            crate::dap::show_ui(self, ctx, &t2);
        }
        {
            let t2 = self.theme_now.clone();
            crate::dap::show_trust_modal(self, ctx, &t2);
        }
        {
            let t = self.theme_now.clone();
            crate::terminal::show(self, ctx, &t);
        }
        crate::bars::show_top(self, ctx);
        crate::bars::show_activity(self, ctx);
        crate::bars::show_sidebar(self, ctx);

        // 5) central editor (+ welcome page) then overlays
        // v2.1: while a file compare is open it REPLACES the editor area
        if self.diff.is_some() {
            crate::diffview::show(self, ctx);
        } else {
            crate::editor::show(self, ctx);
        }
        {
            let t = self.theme_now.clone();
            crate::overlay::show(self, ctx, &t);
        }
        crate::bars::show_about(self, ctx);
        {
            let t = self.theme_now.clone();
            crate::github::show(self, ctx, &t);
        }
        // 5b) v1.5 modals: unsaved-changes confirm (tab + exit),
        // conditional breakpoint / new-file / rename / delete dialogs
        crate::bars::show_modals(self, ctx, &self.theme_now.clone());

        // v2.1: document statistics window
        if self.stats_open {
            let t = self.theme_now.clone();
            crate::texttools::show_stats_window(self, ctx, &t);
        }

        // 6) window title
        let title = match self.active {
            Some(i) => {
                let d = &self.docs[i];
                let dirty = if d.dirty { " •" } else { "" };
                format!("{}{} — REA Studio", d.name, dirty)
            }
            None => "REA Studio".to_string(),
        };
        if title != self.last_title {
            self.last_title = title.clone();
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        }

        // 7) keep status message alive with repaints
        if self.status.1.is_some() {
            ctx.request_repaint_after(Duration::from_millis(300));
        }
    }

    /// Persist undo history + settings for ALL open docs on exit (crash-proof).
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // v1.6: shut down the Live Server thread
        self.live.stop();
        let bdir = self.cfg_dir.join("backups");
        for d in self.docs.iter_mut() {
            let key = d.backup_key();
            d.undo.persist(&bdir, &key);
        }
        // v1.9: session (open tabs + active caret) for the next start
        self.save_session();
        self.save_settings();
    }
}

// used by editor.rs for current-line highlight tinting
#[allow(dead_code)]
fn tint(c: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), alpha)
}

/// v1.9: dotted-version compare — "1.10.0" > "1.9.2"
fn version_newer(a: &str, b: &str) -> bool {
    fn parts(v: &str) -> Vec<u64> {
        v.trim()
            .trim_start_matches(['v', 'V'])
            .split('.')
            .map(|p| {
                p.chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>()
                    .parse::<u64>()
                    .unwrap_or(0)
            })
            .collect()
    }
    let (pa, pb) = (parts(a), parts(b));
    for i in 0..3 {
        let x = pa.get(i).copied().unwrap_or(0);
        let y = pb.get(i).copied().unwrap_or(0);
        if x != y {
            return x > y;
        }
    }
    false
}

// keep icon crate referenced even in release builds
#[allow(dead_code)]
fn _icons_ref(a: icons::Act) -> icons::Act {
    a
}
