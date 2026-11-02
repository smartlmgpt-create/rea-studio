use eframe::egui;
use egui::{Color32, Rounding, Sense, vec2};

use crate::app::App;
use crate::app::ThemeKey;
use crate::theme::Theme;

// ====================================================================
// 20 built-in OFFLINE extensions (theme packs) — shipped inside the exe,
// installed/uninstalled locally without any internet connection.
// ====================================================================

pub struct ThemeSpec {
    pub name: &'static str,
    pub dark: bool,
    pub bg: &'static str,
    pub panel: &'static str,
    pub panel2: &'static str,
    pub editor: &'static str,
    pub tab: &'static str,
    pub tab_act: &'static str,
    pub text: &'static str,
    pub dim: &'static str,
    pub accent: &'static str,
    pub sel: &'static str,
}

pub const BUILTIN_THEMES: &[ThemeSpec] = &[
    ThemeSpec { name: "Dracula", dark: true, bg: "#282a36", panel: "#21222c", panel2: "#191a21", editor: "#282a36", tab: "#21222c", tab_act: "#282a36", text: "#f8f8f2", dim: "#8b93b8", accent: "#bd93f9", sel: "#44475a" },
    ThemeSpec { name: "One Dark Pro", dark: true, bg: "#282c34", panel: "#21252b", panel2: "#181a1f", editor: "#282c34", tab: "#21252b", tab_act: "#282c34", text: "#abb2bf", dim: "#7f848e", accent: "#61afef", sel: "#3e4451" },
    ThemeSpec { name: "Nord", dark: true, bg: "#2e3440", panel: "#292e39", panel2: "#242933", editor: "#2e3440", tab: "#292e39", tab_act: "#2e3440", text: "#d8dee9", dim: "#7b88a1", accent: "#88c0d0", sel: "#434c5e" },
    ThemeSpec { name: "Monokai Classic", dark: true, bg: "#272822", panel: "#1e1f1c", panel2: "#171812", editor: "#272822", tab: "#1e1f1c", tab_act: "#272822", text: "#f8f8f2", dim: "#90918b", accent: "#a6e22e", sel: "#49483e" },
    ThemeSpec { name: "Solarized Dark", dark: true, bg: "#002b36", panel: "#073642", panel2: "#00212b", editor: "#002b36", tab: "#073642", tab_act: "#002b36", text: "#93a1a1", dim: "#657b83", accent: "#268bd2", sel: "#0a4655" },
    ThemeSpec { name: "Solarized Light", dark: false, bg: "#fdf6e3", panel: "#eee8d5", panel2: "#e3ddc8", editor: "#fdf6e3", tab: "#eee8d5", tab_act: "#fdf6e3", text: "#657b83", dim: "#93a1a1", accent: "#268bd2", sel: "#dcd6c1" },
    ThemeSpec { name: "GitHub Dark", dark: true, bg: "#0d1117", panel: "#161b22", panel2: "#010409", editor: "#0d1117", tab: "#161b22", tab_act: "#0d1117", text: "#c9d1d9", dim: "#8b949e", accent: "#58a6ff", sel: "#1f4663" },
    ThemeSpec { name: "GitHub Light", dark: false, bg: "#ffffff", panel: "#f6f8fa", panel2: "#eaeef2", editor: "#ffffff", tab: "#f6f8fa", tab_act: "#ffffff", text: "#1f2328", dim: "#6e7781", accent: "#0969da", sel: "#ddf4ff" },
    ThemeSpec { name: "Tokyo Night", dark: true, bg: "#1a1b26", panel: "#16161e", panel2: "#101014", editor: "#1a1b26", tab: "#16161e", tab_act: "#1a1b26", text: "#a9b1d6", dim: "#565f89", accent: "#7aa2f7", sel: "#292e42" },
    ThemeSpec { name: "Gruvbox Dark", dark: true, bg: "#282828", panel: "#1d2021", panel2: "#161819", editor: "#282828", tab: "#1d2021", tab_act: "#282828", text: "#ebdbb2", dim: "#928374", accent: "#fe8019", sel: "#504945" },
    ThemeSpec { name: "Gruvbox Light", dark: false, bg: "#fbf1c7", panel: "#ebdbb2", panel2: "#d5c4a1", editor: "#fbf1c7", tab: "#ebdbb2", tab_act: "#fbf1c7", text: "#3c3836", dim: "#7c6f64", accent: "#b57614", sel: "#f2e5bc" },
    ThemeSpec { name: "Ayu Dark", dark: true, bg: "#0b0e14", panel: "#11151c", panel2: "#0d1016", editor: "#0b0e14", tab: "#11151c", tab_act: "#0b0e14", text: "#bfbdb6", dim: "#6c7380", accent: "#ffb454", sel: "#1f2532" },
    ThemeSpec { name: "Ayu Light", dark: false, bg: "#fafafa", panel: "#f0f1f2", panel2: "#e7e9ea", editor: "#fafafa", tab: "#f0f1f2", tab_act: "#fafafa", text: "#5c6166", dim: "#8a9199", accent: "#ff9940", sel: "#e9f2f8" },
    ThemeSpec { name: "Rosé Pine", dark: true, bg: "#191724", panel: "#1f1d2e", panel2: "#26233a", editor: "#191724", tab: "#1f1d2e", tab_act: "#191724", text: "#e0def4", dim: "#908caa", accent: "#c4a7e7", sel: "#403d52" },
    ThemeSpec { name: "Midnight Blue", dark: true, bg: "#0d1b2a", panel: "#102a43", panel2: "#0b1d30", editor: "#0d1b2a", tab: "#102a43", tab_act: "#0d1b2a", text: "#d9e2ec", dim: "#627d98", accent: "#4cc2ff", sel: "#1b3a5b" },
    ThemeSpec { name: "Everforest", dark: true, bg: "#2d353b", panel: "#272e33", panel2: "#22282d", editor: "#2d353b", tab: "#272e33", tab_act: "#2d353b", text: "#d3c6aa", dim: "#859289", accent: "#a7c080", sel: "#414b50" },
    ThemeSpec { name: "Synthwave 84", dark: true, bg: "#241b2f", panel: "#2a2139", panel2: "#1a1423", editor: "#241b2f", tab: "#2a2139", tab_act: "#241b2f", text: "#f7f7fb", dim: "#848bbd", accent: "#ff7edb", sel: "#34294f" },
    ThemeSpec { name: "Horizon", dark: true, bg: "#1c1e26", panel: "#16161c", panel2: "#1a1c23", editor: "#1c1e26", tab: "#16161c", tab_act: "#1c1e26", text: "#d5d8da", dim: "#8b8d97", accent: "#e95678", sel: "#2e303e" },
    ThemeSpec { name: "Oceanic Next", dark: true, bg: "#1b2b34", panel: "#152530", panel2: "#102028", editor: "#1b2b34", tab: "#152530", tab_act: "#1b2b34", text: "#c0c5ce", dim: "#65737e", accent: "#6699cc", sel: "#343d46" },
    ThemeSpec { name: "Warm Coffee", dark: true, bg: "#2d2823", panel: "#251f1b", panel2: "#1c1713", editor: "#2d2823", tab: "#251f1b", tab_act: "#2d2823", text: "#e6ddd3", dim: "#a08d7a", accent: "#e6a96b", sel: "#3f3730" },
];

/// Expand a compact ThemeSpec into a full Theme, deriving secondary colors.
pub fn spec_to_theme(s: &ThemeSpec) -> Theme {
    let c = |h: &str| Color32::from_hex(h).unwrap_or(Color32::GRAY);
    let (bg, panel, text, dim, accent) = (c(s.bg), c(s.panel), c(s.text), c(s.dim), c(s.accent));
    let accent_text = if crate::theme::lum(accent) > 0.6 {
        Color32::from_rgb(26, 26, 26)
    } else {
        Color32::WHITE
    };
    Theme {
        name: s.name.to_string(),
        dark: s.dark,
        bg,
        panel,
        panel2: c(s.panel2),
        editor_bg: c(s.editor),
        gutter_bg: c(s.editor),
        tab_bg: c(s.tab),
        tab_active_bg: c(s.tab_act),
        text,
        text_dim: dim,
        text_faint: crate::theme::mix(dim, bg, 0.35),
        accent,
        accent_text,
        border: crate::theme::mix(panel, text, if s.dark { 0.14 } else { 0.18 }),
        hover: crate::theme::mix(panel, text, 0.07),
        widget: crate::theme::mix(panel, text, 0.16),
        active_row: c(s.sel),
        find_bg: panel,
        shadow_col: Color32::from_black_alpha(if s.dark { 150 } else { 60 }),
        ok: if s.dark { c("#4ec9b0") } else { c("#188038") },
        warn: if s.dark { c("#dcdcaa") } else { c("#ea8600") },
        err: if s.dark { c("#f14c4c") } else { c("#d93025") },
    }
}

pub fn spec_installed(app: &App, name: &str) -> bool {
    app.themes_dir().join(format!("{name}.json")).exists()
}

pub fn install_spec(app: &mut App, s: &ThemeSpec) {
    let th = spec_to_theme(s);
    let path = app.themes_dir().join(format!("{}.json", s.name));
    match std::fs::write(&path, crate::theme::to_json(&th)) {
        Ok(_) => {
            app.reload_user_themes();
            app.set_status(format!("Extension installed: {} — Apply to activate", s.name));
        }
        Err(e) => app.set_status(format!("Install failed: {e}")),
    }
}

pub fn uninstall_spec(app: &mut App, name: &str) {
    let path = app.themes_dir().join(format!("{name}.json"));
    app.uninstall_theme(&path);
}

// ====================================================================
// Extensions panel
// ====================================================================

/// Offline extensions panel: 20 built-in theme packs + user themes + fonts.
/// Everything is stored locally in %APPDATA%\REAStudio — 100% offline.
/// Uninstalling the active font falls back to the program's default font.
pub fn ui(app: &mut App, ui: &mut egui::Ui, t: &Theme) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(6.0);

            // ======================= PLUGINS (v1.6) =======================
            section_header(ui, "PLUGINS", t);
            crate::plugins::ui(app, ui, t);

            ui.label(
                egui::RichText::new(format!(
                    "{} offline extensions included — no internet needed",
                    BUILTIN_THEMES.len()
                ))
                .size(10.5)
                .color(t.text_faint),
            );
            ui.add_space(6.0);

            // ======================= MARKETPLACE (.vsix) =======================
            section_header(ui, "MARKETPLACE", t);
            ui.horizontal(|ui| {
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("Install from .vsix…").size(12.0),
                    ))
                    .clicked()
                {
                    install_vsix_dialog(app);
                }
            });
            ui.label(
                egui::RichText::new(
                    "Install a VS Code extension package (.vsix).\n\
                     Contributed color themes are converted and added to your theme list.",
                )
                .size(10.0)
                .color(t.text_faint),
            );
            ui.add_space(4.0);
            // list installed vsix extensions
            let installed = read_installed(app);
            for ext in &installed {
                let name = ext["name"].as_str().unwrap_or("extension");
                let ver = ext["version"].as_str().unwrap_or("");
                let row = egui::Frame::default()
                    .fill(t.bg.gamma_multiply(0.0))
                    .rounding(Rounding::same(6.0))
                    .inner_margin(egui::Margin::symmetric(6.0, 3.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(name).size(12.5).color(t.text),
                            );
                            ui.label(
                                egui::RichText::new(ver).size(10.0).color(t.text_faint),
                            );
                            if ui
                                .add(egui::Button::new(
                                    egui::RichText::new("Uninstall").size(10.0),
                                ))
                                .clicked()
                            {
                                uninstall_vsix(app, name);
                            }
                        });
                    });
                let _ = row;
            }
            ui.add_space(10.0);

            // ============================ CORE ============================
            section_header(ui, "CORE THEMES", t);
            let (a, _) = theme_row(
                ui,
                t,
                "Dark+",
                true,
                app.theme_key == ThemeKey::Dark,
                false,
            );
            if a {
                app.set_theme_key(ThemeKey::Dark);
            }
            let (a, _) = theme_row(
                ui,
                t,
                "Google Light",
                false,
                app.theme_key == ThemeKey::Light,
                false,
            );
            if a {
                app.set_theme_key(ThemeKey::Light);
            }

            ui.add_space(14.0);

            // ==================== BUILT-IN EXTENSIONS (20) ====================
            section_header(ui, "OFFLINE EXTENSIONS (THEME PACKS)", t);
            for s in BUILTIN_THEMES {
                let installed = spec_installed(app, s.name);
                let active = match &app.theme_key {
                    ThemeKey::Custom(p) => {
                        p.file_name().and_then(|f| f.to_str()) == Some(format!("{}.json", s.name).as_str())
                    }
                    _ => false,
                };
                let (apply, uninstall) = builtin_row(ui, app, t, s, installed, active);
                if apply {
                    let path = app.themes_dir().join(format!("{}.json", s.name));
                    app.set_theme_key(ThemeKey::Custom(path));
                } else if uninstall {
                    uninstall_spec(app, s.name);
                }
            }

            ui.add_space(10.0);

            // user-installed theme files (.json picked from disk)
            let user = app.user_themes.clone();
            if !user.is_empty() {
                section_header(ui, "INSTALLED FROM FILE", t);
                for ut in &user {
                    let path = ut.path.clone();
                    let active =
                        matches!(&app.theme_key, ThemeKey::Custom(p) if p.as_path() == ut.path);
                    let (a, u) = theme_row(
                        ui,
                        t,
                        &ut.theme.name,
                        ut.theme.dark,
                        active,
                        true,
                    );
                    if u {
                        app.uninstall_theme(&path);
                    } else if a {
                        app.set_theme_key(ThemeKey::Custom(path));
                    }
                }
                ui.add_space(4.0);
            }

            if ui
                .add(egui::Button::new(
                    egui::RichText::new("+ Install Theme from File (.json)…").size(12.0),
                ))
                .clicked()
            {
                app.install_theme_dialog();
            }
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new("Theme JSON: {\"name\", \"dark\", colors like \"bg\", \"panel\", \"editor_bg\", \"text\", \"accent\"…}")
                    .size(9.5)
                    .color(t.text_faint),
            );

            ui.add_space(14.0);
            ui.separator();
            ui.add_space(8.0);

            // ============================ FONTS ============================
            section_header(ui, "FONTS", t);

            let current = app.custom_font.clone().unwrap_or_else(|| "Default (REA)".into());
            egui::Frame::default()
                .fill(t.bg)
                .rounding(Rounding::same(8.0))
                .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Active:").size(12.0).color(t.text_dim));
                        ui.label(
                            egui::RichText::new(&current)
                                .size(13.0)
                                .strong()
                                .color(t.accent),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add(egui::Button::new(egui::RichText::new("Use Default").size(11.0)))
                                .clicked()
                            {
                                app.use_default_font();
                            }
                        });
                    });
                });
            ui.add_space(6.0);

            let fonts = app.font_list.clone();
            if fonts.is_empty() {
                ui.label(
                    egui::RichText::new("No custom fonts installed.")
                        .size(12.0)
                        .color(t.text_faint),
                );
            } else {
                for fname in &fonts {
                    font_row(ui, app, t, fname);
                }
            }

            ui.add_space(4.0);
            if ui
                .add(egui::Button::new(
                    egui::RichText::new("+ Install Font (.ttf / .otf)…").size(12.0),
                ))
                .clicked()
            {
                app.install_font_dialog();
            }
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new("Installed fonts replace the whole UI font. Removing the active font falls back to the default.")
                    .size(9.5)
                    .color(t.text_faint),
            );
            ui.add_space(10.0);
        });
}

fn section_header(ui: &mut egui::Ui, title: &str, t: &Theme) {
    ui.label(
        egui::RichText::new(title)
            .size(11.0)
            .strong()
            .color(t.text_dim),
    );
    ui.add_space(4.0);
}

/// A row for a built-in extension: Install / Apply / Uninstall.
fn builtin_row(
    ui: &mut egui::Ui,
    app: &mut App,
    t: &Theme,
    s: &ThemeSpec,
    installed: bool,
    active: bool,
) -> (bool, bool) {
    let mut apply_clicked = false;
    let mut uninstall_clicked = false;
    let _ = egui::Frame::default()
        .fill(if active { t.active_row } else { t.bg })
        .stroke(egui::Stroke::new(1.0, t.border))
        .rounding(Rounding::same(8.0))
        .inner_margin(egui::Margin::symmetric(10.0, 7.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                // two-dot preview from real colors
                let (pos, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                let r = pos;
                let pcol = Color32::from_hex(s.editor).unwrap_or(Color32::GRAY);
                let acol = Color32::from_hex(s.accent).unwrap_or(Color32::GRAY);
                ui.painter().rect_filled(r, 8.0, pcol);
                ui.painter()
                    .circle_filled(r.center() + vec2(-3.0, -3.0), 4.0, acol);
                ui.painter()
                    .circle_filled(r.center() + vec2(3.5, 3.0), 4.0, Color32::from_hex(s.text).unwrap_or(Color32::GRAY));

                let mut txt = egui::RichText::new(s.name).size(13.0);
                if active {
                    txt = txt.strong().color(t.text);
                }
                ui.label(txt);
                if active {
                    ui.label(egui::RichText::new("active").size(10.0).color(t.ok));
                } else if installed {
                    ui.label(egui::RichText::new("installed").size(10.0).color(t.text_faint));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if installed && ui
                        .add(egui::Button::new(egui::RichText::new("Uninstall").size(10.5)))
                        .clicked()
                    {
                        uninstall_clicked = true;
                    }
                    if installed && !active && ui
                        .add(egui::Button::new(egui::RichText::new("Apply").size(10.5)))
                        .clicked()
                    {
                        apply_clicked = true;
                    }
                    if !installed && ui
                        .add(egui::Button::new(
                            egui::RichText::new("Install").size(10.5).strong(),
                        ))
                        .clicked()
                    {
                        install_spec(app, s);
                    }
                });
            });
        });
    (apply_clicked, uninstall_clicked)
}

fn theme_row(
    ui: &mut egui::Ui,
    t: &Theme,
    name: &str,
    dark: bool,
    active: bool,
    can_uninstall: bool,
) -> (bool, bool) {
    let mut apply_clicked = false;
    let mut uninstall_clicked = false;
    let _fr = egui::Frame::default()
        .fill(if active { t.active_row } else { t.bg })
        .stroke(stroke1(t.border))
        .rounding(Rounding::same(8.0))
        .inner_margin(egui::Margin::symmetric(10.0, 7.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                // color preview dot
                let (pos, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                let r = pos;
                let (a, b2) = if dark {
                    (Color32::from_rgb(30, 30, 30), Color32::from_rgb(10, 126, 202))
                } else {
                    (Color32::WHITE, Color32::from_rgb(26, 115, 232))
                };
                ui.painter().rect_filled(r, 8.0, a);
                ui.painter()
                    .circle_filled(r.center() + vec2(-3.0, -3.0), 4.0, b2);
                ui.painter()
                    .circle_filled(r.center() + vec2(3.5, 3.0), 4.0, t.text);

                let mut txt = egui::RichText::new(name).size(13.0);
                if active {
                    txt = txt.strong().color(t.text);
                }
                ui.label(txt);
                if active {
                    ui.label(
                        egui::RichText::new("active")
                            .size(10.0)
                            .color(t.ok),
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if can_uninstall {
                        if ui
                            .add(egui::Button::new(egui::RichText::new("Uninstall").size(10.5)))
                            .clicked()
                        {
                            uninstall_clicked = true;
                        }
                    }
                    if !active {
                        if ui
                            .add(egui::Button::new(egui::RichText::new("Apply").size(10.5)))
                            .clicked()
                        {
                            apply_clicked = true;
                        }
                    }
                });
            });
        });
    (apply_clicked, uninstall_clicked)
}

fn font_row(ui: &mut egui::Ui, app: &mut App, t: &Theme, fname: &str) {
    let active = app.custom_font.as_deref() == Some(fname);
    let name = fname.to_string();
    let _fr = egui::Frame::default()
        .fill(if active { t.active_row } else { t.bg })
        .stroke(stroke1(t.border))
        .rounding(Rounding::same(8.0))
        .inner_margin(egui::Margin::symmetric(10.0, 7.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (pos, _) = ui.allocate_exact_size(vec2(20.0, 16.0), Sense::hover());
                let r = pos;
                ui.painter().text(
                    r.center(),
                    egui::Align2::CENTER_CENTER,
                    "Aa",
                    egui::FontId::proportional(11.0),
                    t.accent,
                );
                let mut txt = egui::RichText::new(&name).size(13.0);
                if active {
                    txt = txt.strong();
                }
                ui.label(txt);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(egui::Button::new(egui::RichText::new("Uninstall").size(10.5)))
                        .clicked()
                    {
                        app.uninstall_font(&name);
                    }
                    if !active {
                        if ui
                            .add(egui::Button::new(egui::RichText::new("Use").size(10.5)))
                            .clicked()
                        {
                            app.apply_font(&name);
                        }
                    }
                });
            });
        });
}

// small helper to keep Stroke::new readable in builder chains
fn stroke1(c: Color32) -> egui::Stroke {
    egui::Stroke::new(1.0, c)
}

// ====================================================================
// .vsix installer (VS Code extension packages)
// A .vsix is a ZIP; we extract it with the OS PowerShell (Expand-Archive),
// read extension/package.json, and register any contributed color themes
// by converting the VS Code color tokens into REA themes.
// ====================================================================

/// Sanitize a string coming from an untrusted package.json so it can be
/// used as ONE path component. Rejects path separators, `..`, control
/// characters and empties (prevents writing outside the themes dir).
fn safe_component(s: &str) -> Option<String> {
    let s = s.trim();
    if s.is_empty() || s == "." || s == ".." {
        return None;
    }
    if s.contains('\\') || s.contains('/') || s.contains('\0') {
        return None;
    }
    if s.chars().any(|c| c.is_control()) {
        return None;
    }
    if s.len() > 64 {
        return None;
    }
    Some(s.to_string())
}

/// Pick + install a .vsix (runs the unzip synchronously — packages are small).
pub fn install_vsix_dialog(app: &mut App) {
    let Some(picked) = rfd::FileDialog::new()
        .add_filter("VS Code extension (.vsix)", &["vsix"])
        .pick_file()
    else {
        return;
    };
    app.set_status("Extracting .vsix…");
    let tmp = std::env::temp_dir().join(format!("rea_vsix_{:x}", crate::util::fnv(&picked.display().to_string())));
    let _ = std::fs::remove_dir_all(&tmp);
    let ps = format!(
        "Expand-Archive -Path '{}' -DestinationPath '{}' -Force",
        picked.display().to_string().replace('\'', "''"),
        tmp.display().to_string().replace('\'', "''"),
    );
    let out = std::process::Command::new("powershell.exe")
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", &ps])
        .output();
    match out {
        Ok(o) if o.status.success() => {}
        Ok(o) => {
            app.set_status(format!(
                "Extraction failed: {}",
                String::from_utf8_lossy(&o.stderr).chars().take(120).collect::<String>()
            ));
            return;
        }
        Err(e) => {
            app.set_status(format!("PowerShell failed: {e}"));
            return;
        }
    }

    // find package.json (usually extension/package.json)
    let pkg_candidates = [
        tmp.join("extension").join("package.json"),
        tmp.join("package.json"),
    ];
    let pkg = pkg_candidates.iter().find(|p| p.exists());
    let Some(pkg) = pkg else {
        app.set_status("Not a valid .vsix (no extension/package.json)");
        let _ = std::fs::remove_dir_all(&tmp);
        return;
    };
    let Ok(json) = std::fs::read_to_string(pkg) else {
        app.set_status("Cannot read package.json");
        return;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&json) else {
        app.set_status("package.json is not valid JSON");
        return;
    };
    // SECURITY: `name`/`version` come from the package.json inside the vsix —
    // an attacker-controlled file. Without sanitization `name` like
    // `..\\..\\Startup` escaped the themes dir (arbitrary file write).
    let name = safe_component(v["name"].as_str().unwrap_or("extension"))
        .unwrap_or_else(|| "extension".to_string());
    let version = safe_component(v["version"].as_str().unwrap_or("0.0.0"))
        .unwrap_or_else(|| "0.0.0".to_string());

    // register contributed themes
    let mut themes_added = 0usize;
    if let Some(themes) = v.pointer("/contributes/themes").and_then(|x| x.as_array()) {
        let ext_dir = pkg.parent().unwrap_or(&tmp);
        for thm in themes {
            let Some(path) = thm["path"].as_str() else { continue };
            let theme_path = ext_dir.join(path);
            // SECURITY: a contributed theme path like `..\\..\\secrets` must
            // not make us read files outside the extracted package
            let contained = std::fs::canonicalize(&theme_path)
                .map(|cp| cp.starts_with(std::fs::canonicalize(ext_dir).unwrap_or_else(|_| ext_dir.to_path_buf())))
                .unwrap_or(false);
            if !contained {
                continue;
            }
            let Ok(theme_json) = std::fs::read_to_string(&theme_path) else {
                continue;
            };
            match crate::theme::from_vscode(&theme_json) {
                Ok(mut th) => {
                    let label = thm["label"]
                        .as_str()
                        .map(String::from)
                        .or_else(|| thm["id"].as_str().map(String::from))
                        .unwrap_or_else(|| name.clone());
                    th.name = format!("{label}");
                    let fname = format!(
                        "{}-{}.json",
                        name,
                        crate::util::fnv(&label)
                    );
                    let dest = app.themes_dir().join(fname);
                    if std::fs::write(&dest, crate::theme::to_json(&th)).is_ok() {
                        themes_added += 1;
                    }
                }
                Err(_) => continue,
            }
        }
    }
    app.reload_user_themes();

    // record the extension
    let mut installed = read_installed(app);
    installed.retain(|e| e["name"].as_str() != Some(name.as_str()));
    installed.push(serde_json::json!({
        "name": name, "version": version,
        "publisher": v["publisher"].as_str().unwrap_or(""),
        "themes": themes_added,
    }));
    let _ = std::fs::write(
        installed_path(app),
        serde_json::to_string_pretty(&installed).unwrap_or_default(),
    );
    let _ = std::fs::remove_dir_all(&tmp);
    if themes_added > 0 {
        app.set_status(format!("Installed {name} — {themes_added} theme(s) added"));
    } else {
        app.set_status(format!("Installed {name} (no convertible themes found)"));
    }
}

fn installed_path(app: &App) -> std::path::PathBuf {
    app.cfg_dir.join("installed_vsix.json")
}

fn read_installed(app: &App) -> Vec<serde_json::Value> {
    std::fs::read_to_string(installed_path(app))
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<serde_json::Value>>(&s).ok())
        .unwrap_or_default()
}

fn uninstall_vsix(app: &mut App, name: &str) {
    let mut installed = read_installed(app);
    installed.retain(|e| e["name"].as_str() != Some(name));
    let _ = std::fs::write(
        installed_path(app),
        serde_json::to_string_pretty(&installed).unwrap_or_default(),
    );
    // remove its themes
    let prefix = format!("{name}-");
    if let Ok(rd) = std::fs::read_dir(app.themes_dir()) {
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().starts_with(&prefix) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    app.reload_user_themes();
    app.set_status(format!("Uninstalled {name}"));
}
