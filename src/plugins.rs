//! PLUGIN SYSTEM (v1.6)
//!
//! Built-in plugins are registered here with metadata + enable/disable
//! toggles persisted in settings.json. The manager UI lives in the
//! Extensions side panel (plugins section at the top).

use std::process::Command;

use eframe::egui;

use crate::app::App;
use crate::theme::Theme;

pub struct PluginInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub version: &'static str,
    pub author: &'static str,
    pub desc: &'static str,
    /// commands this plugin contributes to the Command Palette
    pub commands: &'static [&'static str],
}

pub const PLUGINS: &[PluginInfo] = &[
    PluginInfo {
        id: "open-browser",
        name: "Open in Default Browser",
        version: "1.0.0",
        author: "REA Studio",
        desc: "Opens the active file in your system browser — instantly preview HTML, Markdown, PDFs and images.",
        commands: &["Open in Default Browser"],
    },
    PluginInfo {
        id: "live-server",
        name: "Live Server",
        version: "1.1.0",
        author: "REA Studio",
        desc: "Local dev server with automatic browser reload whenever ANY file changes (html/css/js/images/fonts/…). Optional HTTPS with a self-signed certificate.",
        commands: &["Live Server: Start", "Live Server: Stop"],
    },
    PluginInfo {
        id: "rest-client",
        name: "REST Client",
        version: "1.1.0",
        author: "REA Studio",
        desc: "Test APIs via .http files: variables (@var), environments (rea-rest-env.json), named requests + response captures, OAuth2 (client_credentials / password / refresh_token), status/headers/body inspection.",
        commands: &["REST Client: Send Request"],
    },
    PluginInfo {
        id: "code-recommend",
        name: "Code Recommendations",
        version: "1.1.0",
        author: "REA Studio",
        desc: "Smart snippet recommendations while you type (◆ entries in the suggestion popup) for Python, HTML, CSS, JS, Go, Rust, COBOL, PHP, Ruby, Swift, Perl, C#, Kotlin and F# — with real LSP completion when a language server is installed.",
        commands: &["Code Recommendations: Show for Language"],
    },
];

pub fn info(id: &str) -> Option<&'static PluginInfo> {
    PLUGINS.iter().find(|p| p.id == id)
}

impl App {
    pub fn plugin_enabled(&self, id: &str) -> bool {
        *self.settings.plugin_enabled.get(id).unwrap_or(&true)
    }

    pub fn toggle_plugin(&mut self, id: &str) {
        let cur = self.plugin_enabled(id);
        self.settings.plugin_enabled.insert(id.to_string(), !cur);
        self.save_settings();
        if cur {
            // stop side effects when disabling
            match id {
                "live-server" => self.live.stop(),
                _ => {}
            }
            self.set_status(format!("Plugin disabled: {}", info(id).map(|p| p.name).unwrap_or(id)));
        } else {
            self.set_status(format!("Plugin enabled: {}", info(id).map(|p| p.name).unwrap_or(id)));
        }
    }
}

/// Open a file or URL in the default browser / default application.
pub fn open_in_browser(target: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        // SECURITY: the old `cmd /C start "" target` went through cmd's
        // command-string parser, so a filename like `x&calc.exe&.html`
        // executed arbitrary commands. ShellExecuteW talks to the shell
        // association directly — no metacharacter interpretation.
        use std::os::windows::ffi::OsStrExt;
        const SW_SHOWNORMAL: i32 = 1;
        let verb: Vec<u16> = "open\0".encode_utf16().collect();
        let file: Vec<u16> = std::ffi::OsStr::new(target)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let ret = unsafe {
            windows_sys::Win32::UI::Shell::ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                file.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        if (ret as isize) > 32 {
            return Ok(());
        }
        // fallback: explorer handles both paths and URLs, argv-based (no parsing)
        Command::new("explorer")
            .arg(target)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(target)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open")
            .arg(target)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

impl App {
    /// Plugin action: open the ACTIVE DOCUMENT in the default browser.
    pub fn open_active_in_browser(&mut self) {
        if !self.plugin_enabled("open-browser") {
            self.set_status("Open in Browser plugin is disabled (Extensions panel)");
            return;
        }
        let Some(i) = self.active else {
            self.set_status("Open a file first");
            return;
        };
        let Some(path) = self.docs[i].path.clone() else {
            self.set_status("Save the file first (Ctrl+S), then open in browser");
            return;
        };
        if self.docs[i].dirty {
            self.save();
        }
        let url = path.display().to_string();
        match open_in_browser(&url) {
            Ok(_) => self.set_status(format!("Opened in default browser: {}", self.docs[i].name)),
            Err(e) => self.set_status(format!("Browser open failed: {e}")),
        }
    }

    /// Plugin action: start Live Server for the current workspace/file dir.
    pub fn live_start(&mut self) {
        if !self.plugin_enabled("live-server") {
            self.set_status("Live Server plugin is disabled (Extensions panel)");
            return;
        }
        if self.live.running {
            self.set_status(format!("Live Server already running: {}", self.live.url()));
            return;
        }
        let root = self
            .workspace
            .clone()
            .or_else(|| {
                self.active
                    .and_then(|i| self.docs[i].path.clone())
                    .and_then(|p| p.parent().map(|x| x.to_path_buf()))
            })
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        crate::liveserver::set_root(root.clone());
        let https = self.settings.live_https && self.plugin_enabled("live-server");
        let cfg = self.cfg_dir.clone();
        let hp = self.settings.live_https_port;
        match self.live.start(root.clone(), https, hp, &cfg) {
            Ok(port) => {
                // open the browser on the served URL
                let open = self.active.map(|i| self.docs[i].ext()).unwrap_or_default();
                let target = match open.as_str() {
                    "html" | "htm" => {
                        let name = self.docs[self.active.unwrap()].name.clone();
                        format!("http://127.0.0.1:{port}/{}", urlencode(&name))
                    }
                    _ => format!("http://127.0.0.1:{port}/"),
                };
                let _ = open_in_browser(&target);
                self.set_status(format!("Live Server: {target}"));
            }
            Err(e) => self.set_status(format!("Live Server failed: {e}")),
        }
    }

    pub fn live_stop(&mut self) {
        self.live.stop();
        self.set_status("Live Server stopped");
    }
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

// ---------------------------------------------------------------- manager UI

/// v1.7: creator of every language the Code Recommendations plugin ships
/// snippets for — shown with a blue underline next to the language name.
const LANGUAGE_CREATORS: &[(&str, &str)] = &[
    ("Python", "Guido van Rossum"),
    ("HTML", "Tim Berners-Lee"),
    ("CSS", "Håkon Wium Lie"),
    ("JavaScript", "Brendan Eich"),
    ("Go", "Rob Pike · Ken Thompson · Robert Griesemer"),
    ("Rust", "Graydon Hoare"),
    ("COBOL", "Grace Hopper · CODASYL"),
    ("PHP", "Rasmus Lerdorf"),
    ("Ruby", "Yukihiro “Matz” Matsumoto"),
    ("Swift", "Chris Lattner"),
    ("Perl", "Larry Wall"),
    ("C#", "Anders Hejlsberg"),
    ("Kotlin", "JetBrains · Andrey Breslav"),
    ("F#", "Don Syme"),
];

/// Plugins section rendered at the top of the Extensions side panel.
pub fn ui(app: &mut App, ui: &mut egui::Ui, t: &Theme) {
    ui.label(
        egui::RichText::new("PLUGINS")
            .size(10.0)
            .strong()
            .color(t.text_dim),
    );
    ui.add_space(2.0);
    for p in PLUGINS {
        let enabled = app.plugin_enabled(p.id);
        egui::Frame::default()
            .fill(t.panel2)
            .rounding(egui::Rounding::same(7.0))
            .inner_margin(egui::Margin::symmetric(8.0, 6.0))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width() - 4.0);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(p.name)
                            .size(12.0)
                            .strong()
                            .color(if enabled { t.text } else { t.text_faint }),
                    );
                    ui.label(
                        egui::RichText::new(format!("v{}", p.version))
                            .size(9.5)
                            .color(t.text_faint),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let mut on = enabled;
                        if ui
                            .toggle_value(&mut on, egui::RichText::new(if enabled { "ON" } else { "OFF" }).size(9.5))
                            .changed()
                        {
                            app.toggle_plugin(p.id);
                        }
                    });
                });
                ui.label(
                    egui::RichText::new(p.desc)
                        .size(10.0)
                        .color(t.text_dim),
                );
                // plugin-specific quick actions
                ui.horizontal(|ui| {
                    match p.id {
                        "open-browser" => {
                            if ui
                                .add_enabled(enabled, egui::Button::new(egui::RichText::new("Open active file").size(9.5)))
                                .clicked()
                            {
                                app.open_active_in_browser();
                            }
                        }
                        "live-server" => {
                            if app.live.running {
                                if ui.button(egui::RichText::new(format!("Stop :{}", app.live.port)).size(9.5)).clicked() {
                                    app.live_stop();
                                }
                                ui.label(
                                    egui::RichText::new(format!("● {}  ({} served)", app.live.url(), app.live.served()))
                                        .size(9.5)
                                        .color(t.ok),
                                );
                            } else if ui.add_enabled(enabled, egui::Button::new(egui::RichText::new("Start server").size(9.5))).clicked() {
                                app.live_start();
                            }
                        }
                        "rest-client" => {
                            if ui.add_enabled(enabled, egui::Button::new(egui::RichText::new("Open REST panel").size(9.5))).clicked() {
                                app.bottom_tab = crate::app::BottomTab::Rest;
                                app.terminal_visible = true;
                            }
                        }
                        "code-recommend" => {
                            let ext = app.active.map(|i| app.docs[i].ext()).unwrap_or_default();
                            let n = crate::recco::for_ext(&ext).len();
                            let _ = n;
                            if ui.add_enabled(enabled, egui::Button::new(egui::RichText::new("Recommend now (Ctrl+Space)").size(9.5))).clicked() {
                                app.suggest_force_open();
                            }
                            // v1.7: language creators — blue underlined, next to
                            // each language name (like reference links)
                            ui.add_space(3.0);
                            ui.label(
                                egui::RichText::new("LANGUAGES & CREATORS")
                                    .size(8.5)
                                    .strong()
                                    .color(t.text_faint),
                            );
                            for (lang, creator) in LANGUAGE_CREATORS {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(format!("◆ {lang}"))
                                            .size(9.5)
                                            .color(t.text_dim),
                                    );
                                    ui.label(
                                        egui::RichText::new(*creator)
                                            .size(9.5)
                                            .color(egui::Color32::from_rgb(0x4E, 0x9C, 0xD6))
                                            .underline(),
                                    );
                                });
                            }
                        }
                        _ => {}
                    }
                });
            });
        ui.add_space(3.0);
    }
    ui.add_space(6.0);
    ui.separator();
    ui.add_space(2.0);
}
