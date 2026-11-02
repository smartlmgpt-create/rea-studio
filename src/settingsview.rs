//! Settings panel — a real GUI for every editor preference, persisted to
//! settings.json. Includes font size, tab size, wrap, autosave, breadcrumbs,
//! terminal-on-boot and the LSP server commands per language.

use eframe::egui;
use egui::{Align, Rounding, Sense};

use crate::app::{App, PanelMode};
use crate::theme::Theme;

pub fn show(app: &mut App, ui: &mut egui::Ui, t: &Theme) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(6.0);

            // ---------------- editor ----------------
            section(ui, "EDITOR", t);
            let mut fs = app.settings.font_size;
            if ui
                .add(egui::Slider::new(&mut fs, 11.0..=30.0).text("Font size"))
                .changed()
            {
                app.settings.font_size = fs;
                app.apply_font_size();
            }
            let mut ts = app.settings.tab_size as i32;
            if ui
                .add(egui::Slider::new(&mut ts, 2..=8).text("Tab / indent size"))
                .changed()
            {
                app.settings.tab_size = ts.max(1) as usize;
            }
            toggle(ui, "Word wrap (Alt+Z)", &mut app.settings.word_wrap, t);
            toggle(ui, "Auto-save on idle", &mut app.settings.autosave, t);
            toggle(ui, "Breadcrumbs bar", &mut app.settings.breadcrumbs, t);
            toggle(
                ui,
                "Show hidden files (dotfiles)",
                &mut app.settings.show_dotfiles,
                t,
            );
            toggle(
                ui,
                "Show terminal on startup",
                &mut app.settings.terminal_on_boot,
                t,
            );
            toggle(ui, "Bracket auto-pair", &mut app.settings.auto_pair, t);
            toggle(ui, "Auto-indent on Enter", &mut app.settings.auto_indent, t);
            toggle(ui, "Show minimap (v1.7)", &mut app.settings.minimap, t);
            toggle(
                ui,
                "Format on save (rustfmt/black/prettier/gofmt) (v1.7)",
                &mut app.settings.format_on_save,
                t,
            );

            // ---------------- terminal (v1.7) ----------------
            section(ui, "TERMINAL", t);
            toggle(
                ui,
                "Use ConPTY (real console: vim/htop, passwords, y/n) (v1.7)",
                &mut app.settings.terminal_conpty,
                t,
            );
            ui.label(
                egui::RichText::new(
                    "ConPTY needs Windows 10 1809+. When off (or unavailable) the \n\
                     legacy pipe mode is used — plain text, no TUI programs.",
                )
                .size(10.0)
                .color(t.text_faint),
            );

            // ---------------- live server (v1.7) ----------------
            section(ui, "LIVE SERVER", t);
            toggle(
                ui,
                "Serve HTTPS (self-signed cert, browser warning is normal) (v1.7)",
                &mut app.settings.live_https,
                t,
            );
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("HTTPS port:").size(11.0));
                let mut hp = app.settings.live_https_port as i32;
                if ui.add(egui::DragValue::new(&mut hp).range(1024..=65535)).changed() {
                    app.settings.live_https_port = hp as u16;
                }
                ui.label(
                    egui::RichText::new("(reload works over both HTTP and HTTPS)")
                        .size(10.0)
                        .color(t.text_faint),
                );
            });

            // ---------------- language servers ----------------
            section(ui, "LANGUAGE SERVERS (LSP)", t);
            ui.label(
                egui::RichText::new(
                    "One command per extension; the server must be on PATH.\n\
                     Empty = keyword-only completion for that language.",
                )
                .size(10.5)
                .color(t.text_faint),
            );
            ui.add_space(4.0);
            let mut entries: Vec<(String, String)> = app.settings.lsp.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            entries.sort();
            for (ext, cmd) in &entries {
                ui.horizontal(|ui| {
                    ui.monospace(egui::RichText::new(format!("{ext:>5}")).size(11.5).color(t.accent));
                    let mut v = cmd.clone();
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut v)
                            .hint_text("e.g. rust-analyzer")
                            .desired_width(ui.available_width() - 12.0)
                            .font(egui::TextStyle::Monospace),
                    );
                    if r.changed() {
                        if v.trim().is_empty() {
                            app.settings.lsp.remove(ext);
                        } else {
                            app.settings.lsp.insert(ext.clone(), v);
                        }
                    }
                });
            }
            ui.horizontal(|ui| {
                if ui.button("+ rust-analyzer (Rust)").clicked() {
                    app.settings.lsp.insert("rs".into(), "rust-analyzer".into());
                }
                if ui.button("+ pylsp (Python)").clicked() {
                    app.settings.lsp.insert("py".into(), "pylsp".into());
                }
                if ui.button("+ gopls (Go)").clicked() {
                    app.settings.lsp.insert("go".into(), "gopls".into());
                }
            });

            // ---------------- updates (v2.0) ----------------
            section(ui, "UPDATES", t);
            ui.label(
                egui::RichText::new(
                    "Manifest URL checked at startup (JSON: version / url / notes).\n\
                     Empty = built-in default on GitHub.",
                )
                .size(10.5)
                .color(t.text_faint),
            );
            ui.add_space(4.0);
            let mut uu = app.update_url.clone().unwrap_or_default();
            let r = ui.add(
                egui::TextEdit::singleline(&mut uu)
                    .hint_text(App::UPDATE_URL)
                    .desired_width(ui.available_width() - 12.0)
                    .font(egui::TextStyle::Monospace),
            );
            if r.changed() {
                let v = uu.trim().to_string();
                app.update_url = if v.is_empty() { None } else { Some(v) };
                app.save_settings();
            }
            ui.horizontal(|ui| {
                if ui.button("Check now…").clicked() {
                    app.upd_check_now();
                }
                match app.upd.clone() {
                    crate::app::UpdState::Checking => {
                        ui.label(egui::RichText::new("Checking…").size(10.5).color(t.text_faint));
                    }
                    crate::app::UpdState::UpToDate => {
                        ui.label(egui::RichText::new("up to date ✓").size(10.5).color(t.ok));
                    }
                    crate::app::UpdState::Available { ver, .. } => {
                        ui.label(
                            egui::RichText::new(format!("new: v{}", ver)).size(10.5).color(t.warn),
                        );
                    }
                    crate::app::UpdState::Failed => {
                        ui.label(
                            egui::RichText::new("failed (offline / no manifest)")
                                .size(10.5)
                                .color(t.text_faint),
                        );
                    }
                    crate::app::UpdState::Idle => {}
                }
            });

            // ---------------- about data ----------------
            section(ui, "DATA", t);
            ui.horizontal(|ui| {
                if ui.button("Clear recent files").clicked() {
                    app.recents.clear();
                    app.save_settings();
                    app.set_status("Recent files cleared");
                }
                if ui.button("Clear recent folders").clicked() {
                    app.recent_folders.clear();
                    app.save_settings();
                }
                if ui.button("Clear undo backups").clicked() {
                    let d = app.cfg_dir.join("backups");
                    if d.exists() {
                        let _ = std::fs::remove_dir_all(d);
                    }
                    app.set_status("Undo backups cleared");
                }
            });

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button("Reset to defaults").clicked() {
                    app.settings = crate::app::EditorSettings::default();
                    app.apply_font_size();
                    app.save_settings();
                    app.set_status("Settings reset");
                }
                if ui.button("Close (Esc)").clicked() {
                    app.show_sidebar = false;
                    app.panel_mode = PanelMode::Explorer;
                }
            });
            ui.add_space(20.0);
        });
}

fn section(ui: &mut egui::Ui, title: &str, t: &Theme) {
    ui.add_space(6.0);
    egui::Frame::default()
        .fill(t.panel2.gamma_multiply(0.4))
        .rounding(Rounding::same(6.0))
        .inner_margin(egui::Margin::symmetric(8.0, 3.0))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(title).size(10.5).strong().color(t.text_dim));
        });
    ui.add_space(6.0);
}

fn toggle(ui: &mut egui::Ui, label: &str, val: &mut bool, t: &Theme) {
    let r = ui.checkbox(val, label);
    let _ = t;
    let _ = r;
    let _ = Align::LEFT;
    let _ = Sense::hover();
}
