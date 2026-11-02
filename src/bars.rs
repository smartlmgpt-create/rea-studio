use std::path::PathBuf;

use eframe::egui;
use egui::{Align, Align2, Color32, Rounding, Sense, Stroke, Vec2, vec2};

use crate::app::{App, PanelMode};
use crate::icons;
use crate::theme::Theme;

/// A node in the explorer tree.
#[derive(Clone)]
pub struct Node {
    pub is_dir: bool,
    pub name: String,
    pub path: std::path::PathBuf,
    pub ext: String,
}

// ---------------------------------------------------------------- top bar (menu + tabs)

pub fn show_top(app: &mut App, ctx: &egui::Context) {
    let t = app.theme_now.clone();
    egui::TopBottomPanel::top("rea_top_bar")
        .frame(egui::Frame::none().fill(t.panel))
        .show(ctx, |ui| {
            // ---- menu row ----
            ui.horizontal(|ui| {
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new("REA")
                        .size(14.0)
                        .strong()
                        .color(t.accent),
                );
                ui.label(
                    egui::RichText::new("Studio")
                        .size(14.0)
                        .strong()
                        .color(t.text),
                );
                ui.add_space(10.0);

                ui.menu_button("File", |ui| {
                    if ui.button("New File            Ctrl+N").clicked() {
                        app.new_file();
                        ui.close_menu();
                    }
                    if ui.button("Open File           Ctrl+O").clicked() {
                        app.open_file_dialog();
                        ui.close_menu();
                    }
                    if ui.button("Open Folder    Ctrl+Shift+O").clicked() {
                        app.open_folder_dialog();
                        ui.close_menu();
                    }
                    if !app.recent_folders.is_empty() || !app.recents.is_empty() {
                        ui.menu_button("Open Recent", |ui| {
                            if !app.recent_folders.is_empty() {
                                ui.label(egui::RichText::new("FOLDERS").size(10.0).color(t.text_faint));
                                let folders: Vec<PathBuf> = app.recent_folders.clone();
                                for f in folders {
                                    let name = f.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                                    if ui.button(format!("📁 {}", name)).clicked() {
                                        app.set_workspace(f);
                                        ui.close_menu();
                                    }
                                }
                                ui.separator();
                            }
                            ui.label(egui::RichText::new("FILES").size(10.0).color(t.text_faint));
                            let files: Vec<PathBuf> = app.recents.clone();
                            for f in files.iter().take(8) {
                                let name = f.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                                if ui.button(name).clicked() {
                                    let p = f.clone();
                                    app.open_path(&p);
                                    ui.close_menu();
                                }
                            }
                        });
                    }
                    ui.separator();
                    if ui.button("Save                  Ctrl+S").clicked() {
                        app.save();
                        ui.close_menu();
                    }
                    if ui.button("Save As…         Ctrl+Shift+S").clicked() {
                        app.save_as_dialog();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Clone from GitHub…  Ctrl+Shift+G").clicked() {
                        app.open_github_dialog();
                        ui.close_menu();
                    }
                    if ui.button("Save All").clicked() {
                        app.save_all();
                        ui.close_menu();
                    }
                    if ui.button("Restore Last Session").clicked() {
                        app.restore_last_session();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Exit").clicked() {
                        // #12: never process::exit — confirm unsaved changes
                        app.request_exit();
                        ui.close_menu();
                    }
                });

                ui.menu_button("Edit", |ui| {
                    if ui.button("Undo                 Ctrl+Z").clicked() {
                        app.do_undo();
                        ui.close_menu();
                    }
                    if ui.button("Redo                 Ctrl+Y").clicked() {
                        app.do_redo();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Find in File         Ctrl+F").clicked() {
                        app.open_find();
                        ui.close_menu();
                    }
                    if ui.button("Find & Replace       Ctrl+H").clicked() {
                        app.open_find();
                        app.find.show_replace = true;
                        app.find.replace_focus_req = true;
                        ui.close_menu();
                    }
                    if ui.button("Search in Workspace  Ctrl+Shift+F").clicked() {
                        app.panel_mode = PanelMode::Search;
                        app.show_sidebar = true;
                        app.refresh_tree();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Add Next Occurrence  Ctrl+D").clicked() {
                        app.multi_select_next();
                        ui.close_menu();
                    }
                    if ui.button("Select All Occurrences Ctrl+Shift+L").clicked() {
                        app.multi_select_all();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Toggle Comment    Ctrl+/").clicked() {
                        app.toggle_comment();
                        ui.close_menu();
                    }
                    if ui.button("Delete Line       Ctrl+Shift+K").clicked() {
                        app.delete_line();
                        ui.close_menu();
                    }
                    if ui.button("Move Line Up      Alt+Up").clicked() {
                        app.move_line(true);
                        ui.close_menu();
                    }
                    if ui.button("Move Line Down    Alt+Down").clicked() {
                        app.move_line(false);
                        ui.close_menu();
                    }
                    if ui.button("Go to Line        Ctrl+G").clicked() {
                        app.open_goto_line();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Toggle Bookmark    Ctrl+F2").clicked() {
                        app.toggle_bookmark();
                        ui.close_menu();
                    }
                    if ui.button("Next Bookmark            F2").clicked() {
                        app.bookmark_nav(true);
                        ui.close_menu();
                    }
                    if ui.button("Previous Bookmark  Shift+F2").clicked() {
                        app.bookmark_nav(false);
                        ui.close_menu();
                    }
                    if ui.button("Clear Bookmarks     Alt+F2").clicked() {
                        app.bookmark_clear_doc();
                        ui.close_menu();
                    }
                    ui.separator();
                    // ---- v2.0: macro + column editing ----
                    if ui
                        .button(if app.macro_rec.is_some() {
                            "Stop Recording Macro   Ctrl+Shift+R"
                        } else {
                            "Record Macro           Ctrl+Shift+R"
                        })
                        .clicked()
                    {
                        app.macro_record_toggle();
                        ui.close_menu();
                    }
                    if ui.button("Run Macro               Ctrl+Shift+M").clicked() {
                        app.macro_play();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Column Selection  Alt+Shift+Arrows").clicked() {
                        app.col_start();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Go to Definition (LSP)  F12").clicked() {
                        app.lsp_goto_definition();
                        ui.close_menu();
                    }
                });

                ui.menu_button("Run", |ui| {
                    if ui.button("Start Debugging        F5").clicked() {
                        app.debug_start();
                        ui.close_menu();
                    }
                    if ui.button("Stop Debugging     Shift+F5").clicked() {
                        app.debug_stop();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Toggle Breakpoint      F9").clicked() {
                        if let (Some(i), Some((ln, _))) = (app.active, app.cursor) {
                            if let Some(p) = app.docs[i].path.clone() {
                                app.toggle_breakpoint(p, ln);
                            }
                        }
                        ui.close_menu();
                    }
                });

                // ---- v2.1: Tools menu ----
                ui.menu_button("Tools", |ui| {
                    if ui.button("Compare Two Files…        F8 nav").clicked() {
                        crate::diffview::open_dialog(app);
                        ui.close_menu();
                    }
                    if ui.button("TODO Scanner (Workspace)").clicked() {
                        app.panel_mode = PanelMode::Todos;
                        app.show_sidebar = true;
                        crate::todopanel::scan_start(app);
                        ui.close_menu();
                    }
                    if ui.button("Document Statistics…").clicked() {
                        app.doc_stats_open();
                        ui.close_menu();
                    }
                    ui.separator();
                    ui.menu_button("Line Operations", |ui| {
                        if ui.button("Sort Lines A → Z").clicked() {
                            app.tool_run("sort_asc", "Sorted lines A → Z");
                            ui.close_menu();
                        }
                        if ui.button("Sort Lines Z → A").clicked() {
                            app.tool_run("sort_desc", "Sorted lines Z → A");
                            ui.close_menu();
                        }
                        if ui.button("Remove Duplicate Lines").clicked() {
                            app.tool_run("dedupe", "Duplicate lines removed");
                            ui.close_menu();
                        }
                        if ui.button("Remove Empty Lines").clicked() {
                            app.tool_run("rm_empty", "Empty lines removed");
                            ui.close_menu();
                        }
                        if ui.button("Reverse Line Order").clicked() {
                            app.tool_run("reverse", "Line order reversed");
                            ui.close_menu();
                        }
                        if ui.button("Trim Trailing Spaces").clicked() {
                            app.tool_run("trim_trail", "Trailing spaces trimmed");
                            ui.close_menu();
                        }
                        if ui.button("Trim Leading Spaces").clicked() {
                            app.tool_run("trim_lead", "Leading spaces trimmed");
                            ui.close_menu();
                        }
                    });
                    ui.menu_button("Convert Case (selection)", |ui| {
                        if ui.button("UPPERCASE").clicked() {
                            app.tool_run("upper", "Uppercased");
                            ui.close_menu();
                        }
                        if ui.button("lowercase").clicked() {
                            app.tool_run("lower", "Lowercased");
                            ui.close_menu();
                        }
                        if ui.button("Title Case").clicked() {
                            app.tool_run("title", "Title Cased");
                            ui.close_menu();
                        }
                        if ui.button("iNVERT cASE").clicked() {
                            app.tool_run("invert", "Case inverted");
                            ui.close_menu();
                        }
                    });
                    ui.menu_button("Encode / Decode", |ui| {
                        if ui.button("Base64 Encode (selection/word)").clicked() {
                            app.tool_run("b64_enc", "Base64 encoded");
                            ui.close_menu();
                        }
                        if ui.button("Base64 Decode (selection/word)").clicked() {
                            app.tool_run("b64_dec", "Base64 decoded");
                            ui.close_menu();
                        }
                        if ui.button("URL Encode (selection/word)").clicked() {
                            app.tool_run("url_enc", "URL encoded");
                            ui.close_menu();
                        }
                        if ui.button("URL Decode (selection/word)").clicked() {
                            app.tool_run("url_dec", "URL decoded");
                            ui.close_menu();
                        }
                        if ui.button("JSON Escape (selection)").clicked() {
                            app.tool_run("json_esc", "JSON-escaped");
                            ui.close_menu();
                        }
                        if ui.button("JSON Unescape (selection)").clicked() {
                            app.tool_run("json_unesc", "JSON-unescaped");
                            ui.close_menu();
                        }
                    });
                    ui.menu_button("Hash (→ clipboard)", |ui| {
                        if ui.button("MD5 (selection/word)").clicked() {
                            app.tool_run("md5", "MD5");
                            ui.close_menu();
                        }
                        if ui.button("SHA-1 (selection/word)").clicked() {
                            app.tool_run("sha1", "SHA-1");
                            ui.close_menu();
                        }
                        if ui.button("SHA-256 (selection/word)").clicked() {
                            app.tool_run("sha256", "SHA-256");
                            ui.close_menu();
                        }
                    });
                    ui.menu_button("Line Endings / BOM", |ui| {
                        if ui.button("Convert to Unix (LF)").clicked() {
                            app.eol_set(false);
                            ui.close_menu();
                        }
                        if ui.button("Convert to Windows (CRLF)").clicked() {
                            app.eol_set(true);
                            ui.close_menu();
                        }
                        if ui.button("Toggle UTF-8 BOM").clicked() {
                            app.bom_toggle();
                            ui.close_menu();
                        }
                    });
                    ui.separator();
                    if ui.button("Insert Timestamp").clicked() {
                        app.tool_run("timestamp", "Timestamp inserted");
                        ui.close_menu();
                    }
                });

                ui.menu_button("View", |ui| {
                    if ui.button("Command Palette…  Ctrl+Shift+P").clicked() {
                        app.open_palette(crate::overlay::PaletteMode::Commands);
                        ui.close_menu();
                    }
                    if ui.button("Go to File…        Ctrl+P").clicked() {
                        app.open_palette(crate::overlay::PaletteMode::Files);
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button(format!("Word Wrap {}     Alt+Z", if app.settings.word_wrap {"✔"} else {""})).clicked() {
                        app.settings.word_wrap = !app.settings.word_wrap;
                        app.save_settings();
                        ui.close_menu();
                    }
                    if ui.button(format!("Breadcrumbs {}", if app.settings.breadcrumbs {"✔"} else {""})).clicked() {
                        app.settings.breadcrumbs = !app.settings.breadcrumbs;
                        app.save_settings();
                        ui.close_menu();
                    }
                    if ui.button(format!("Auto-save {}", if app.settings.autosave {"✔"} else {""})).clicked() {
                        app.settings.autosave = !app.settings.autosave;
                        app.save_settings();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Toggle Explorer    Ctrl+B").clicked() {
                        app.toggle_explorer();
                        ui.close_menu();
                    }
                    if ui.button("Function List (Outline)").clicked() {
                        app.click_panel(crate::app::PanelMode::Outline);
                        ui.close_menu();
                    }
                    if ui.button("Toggle Terminal    Ctrl+`").clicked() {
                        app.toggle_terminal();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Zoom In              Ctrl+=" ).clicked() {
                        app.zoom_step(1.15);
                        ui.close_menu();
                    }
                    if ui.button("Zoom Out             Ctrl+-").clicked() {
                        app.zoom_step(1.0 / 1.15);
                        ui.close_menu();
                    }
                    if ui.button("Reset Zoom           Ctrl+0").clicked() {
                        app.zoom = 1.0;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button(format!("Restore Session {}", if app.session_restore {"✔"} else {""})).clicked() {
                        app.session_restore = !app.session_restore;
                        app.save_settings();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Settings           Ctrl+,").clicked() {
                        app.panel_mode = PanelMode::Settings;
                        app.show_sidebar = true;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Dark+ Theme").clicked() {
                        app.set_theme_key(crate::app::ThemeKey::Dark);
                        ui.close_menu();
                    }
                    if ui.button("Google Light Theme").clicked() {
                        app.set_theme_key(crate::app::ThemeKey::Light);
                        ui.close_menu();
                    }
                });

                ui.menu_button("Terminal", |ui| {
                    if ui.button("Toggle Terminal     Ctrl+`").clicked() {
                        app.toggle_terminal();
                        ui.close_menu();
                    }
                    if ui.button("Clear").clicked() {
                        app.clear_terminal();
                        ui.close_menu();
                    }
                    if ui.button("Restart PowerShell").clicked() {
                        app.restart_terminal();
                        ui.close_menu();
                    }
                });

                ui.menu_button("Help", |ui| {
                    if ui.button("Welcome").clicked() {
                        app.close_all_docs();
                        ui.close_menu();
                    }
                    if ui.button("Check for Updates…").clicked() {
                        app.upd_check_now();
                        ui.close_menu();
                    }
                    // v1.9: silent startup probe result
                    match app.upd.clone() {
                        crate::app::UpdState::Checking => {
                            ui.label(egui::RichText::new("Checking…").size(11.0).color(t.text_faint));
                        }
                        crate::app::UpdState::UpToDate => {
                            ui.label(egui::RichText::new(concat!("v", env!("CARGO_PKG_VERSION"), " — up to date ✓"))
                                .size(11.0).color(t.ok));
                        }
                        crate::app::UpdState::Available { ver, url, notes } => {
                            ui.label(egui::RichText::new(format!("New version v{} available!", ver))
                                .size(12.0).strong().color(t.warn));
                            if !notes.is_empty() {
                                ui.label(egui::RichText::new(&notes).size(10.5).color(t.text_dim));
                            }
                            if !url.is_empty() && ui.button("Download…").clicked() {
                                app.open_url(&url);
                                ui.close_menu();
                            }
                        }
                        crate::app::UpdState::Failed => {
                            ui.label(egui::RichText::new("Update check failed (offline?)")
                                .size(11.0).color(t.text_faint));
                            // v2.0: tell the user HOW to make the check work —
                            // the default manifest lives on GitHub; if it is
                            // missing (repo not created / blocked), point at
                            // the settings.json override instead of failing
                            // silently.
                            if app.update_url.is_none() {
                                ui.label(egui::RichText::new(
                                    "No manifest reachable. Host latest.json on a\n\
                                     repo or set \"update_url\" in settings.json.",
                                ).size(10.0).color(t.text_faint));
                            }
                        }
                        crate::app::UpdState::Idle => {}
                    }
                    ui.separator();
                    if ui.button("About REA Studio").clicked() {
                        app.about_open = true;
                        ui.close_menu();
                    }
                });

                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new("100% Rust")
                            .size(10.5)
                            .color(t.text_faint),
                    );
                });
            });

            ui.add_space(2.0);

            // ---- tab row (v1.7: drag & drop reorder) ----
            if !app.docs.is_empty() {
                ui.separator();
                app.tab_rects.clear();
                egui::ScrollArea::horizontal()
                    .id_salt("tabs")
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let docs: Vec<(String, bool, String)> = app
                                .docs
                                .iter()
                                .map(|d| (d.name.clone(), d.dirty, d.ext()))
                                .collect();
                            for (i, (name, dirty, ext)) in docs.iter().enumerate() {
                                let active = app.active == Some(i);
                                let dragging = app.tab_drag == Some(i);
                                let fr = egui::Frame::default()
                                    .fill(if active || dragging { t.tab_active_bg } else { Color32::TRANSPARENT })
                                    .rounding(Rounding { nw: 8.0, ne: 8.0, sw: 0.0, se: 0.0 })
                                    .inner_margin(egui::Margin::symmetric(8.0, 5.0))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            icons::badge(ui, ext);
                                            let mut txt =
                                                egui::RichText::new(name.as_str()).size(13.0);
                                            txt = if active {
                                                txt.color(t.text)
                                            } else {
                                                txt.color(t.text_dim)
                                            };
                                            ui.label(txt);
                                            if *dirty {
                                                ui.label(
                                                    egui::RichText::new("•")
                                                        .size(13.0)
                                                        .color(t.accent),
                                                );
                                            }
                                            let (xp, xr) = ui.allocate_exact_size(
                                                vec2(16.0, 16.0),
                                                Sense::click(),
                                            );
                                            if xr.hovered() {
                                                ui.painter().rect_filled(
                                                    xp,
                                                    4.0,
                                                    t.hover,
                                                );
                                            }
                                            icons::close_x(
                                                &ui.painter(),
                                                xp
                                                    .center(),
                                                t.text_dim,
                                            );
                                            if xr.clicked() {
                                                // #12: dirty tabs ask before closing
                                                app.request_close_doc(i);
                                            }
                                        });
                                    })
                                    .response
                                    .interact(Sense::click_and_drag());
                                app.tab_rects.push((i, fr.rect));
                                if fr.drag_started() {
                                    app.tab_drag = Some(i);
                                }
                                if fr.clicked() && app.tab_drag.is_none() {
                                    app.active = Some(i);
                                }
                                if fr.hovered() && app.tab_drag.is_some() && app.tab_drag != Some(i) {
                                    ui.painter().rect_stroke(fr.rect, 6.0, Stroke::new(1.4, t.accent));
                                }
                                if dragging {
                                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                                }
                                ui.add_space(2.0);
                            }
                        });
                    });

                // ---- drag & drop drop logic (after the tab row was laid out) ----
                if let Some(from) = app.tab_drag {
                    let released = ui.input(|i| i.pointer.any_released());
                    let pos = ui.input(|i| i.pointer.interact_pos());
                    if released {
                        let target = pos
                            .and_then(|p| {
                                app.tab_rects
                                    .iter()
                                    .find(|(i, r)| *i != from && r.contains(p))
                                    .map(|(i, _)| *i)
                            });
                        if let (Some(target), true) = (target, from < app.docs.len() && target.map(|x| x < app.docs.len()).unwrap_or(false)) {
                            let doc = app.docs.remove(from);
                            app.docs.insert(target, doc);
                            app.active = Some(target);
                            // keep the split pane doc index in sync
                            if let Some(sd) = app.split_doc {
                                let sd = if sd == from { target } else if sd > from && sd <= target { sd - 1 } else if sd < from && sd >= target { sd + 1 } else { sd };
                                app.split_doc = Some(sd);
                            }
                        }
                        app.tab_drag = None;
                    }
                }
            }
        });
}

// ---------------------------------------------------------------- activity bar

pub fn show_activity(app: &mut App, ctx: &egui::Context) {
    let t = app.theme_now.clone();
    egui::SidePanel::left("rea_activity")
        .exact_width(46.0)
        .frame(egui::Frame::none().fill(t.panel2))
        .show(ctx, |ui| {
            ui.add_space(8.0);
            for (act, mode, _tip) in [
                (icons::Act::Explorer, PanelMode::Explorer, "Explorer"),
                (icons::Act::Search, PanelMode::Search, "Search (files & content)"),
                (icons::Act::Todo, PanelMode::Todos, "TODO Scanner (workspace)"),
                (icons::Act::Outline, PanelMode::Outline, "Function List (Outline)"),
                (icons::Act::Extensions, PanelMode::Extensions, "Extensions"),
                (icons::Act::Settings, PanelMode::Settings, "Settings"),
            ] {
                let selected = app.show_sidebar && app.panel_mode == mode;
                let col = if selected { t.accent } else { t.text_dim };
                if icons::activity(ui, act, col).clicked() {
                    app.click_panel(mode);
                }
                ui.add_space(6.0);
            }

            ui.add_space(10.0);
            let tcol = if app.terminal_visible { t.accent } else { t.text_dim };
            if icons::activity(ui, icons::Act::Terminal, tcol).clicked() {
                app.toggle_terminal();
            }
            ui.add_space(6.0);
            if icons::activity(ui, icons::Act::Theme, t.text_dim).clicked() {
                app.toggle_theme();
            }

            ui.with_layout(egui::Layout::bottom_up(Align::Center), |ui| {
                ui.add_space(8.0);
                if icons::activity(ui, icons::Act::Info, t.text_dim).clicked() {
                    app.about_open = true;
                }
            });
        });
}

// ---------------------------------------------------------------- sidebar (explorer / extensions / search)

pub fn show_sidebar(app: &mut App, ctx: &egui::Context) {
    if !app.show_sidebar {
        return;
    }
    let t = app.theme_now.clone();
    egui::SidePanel::left("rea_sidebar")
        .resizable(true)
        .default_width(250.0)
        .width_range(170.0..=460.0)
        .frame(egui::Frame::none().fill(t.panel))
        .show(ctx, |ui| {
            match app.panel_mode {
                PanelMode::Explorer => {
                    header(ui, "EXPLORER", t.panel, t.text_dim);
                    explorer_ui(app, ui, &t);
                }
                PanelMode::Extensions => {
                    header(ui, "EXTENSIONS", t.panel, t.text_dim);
                    crate::extensions::ui(app, ui, &t);
                }
                PanelMode::Settings => {
                    header(ui, "SETTINGS", t.panel, t.text_dim);
                    crate::settingsview::show(app, ui, &t);
                }
                PanelMode::Search => {
                    header(ui, "SEARCH", t.panel, t.text_dim);
                    search_ui(app, ui, &t);
                }
                PanelMode::Outline => {
                    header(ui, "OUTLINE", t.panel, t.text_dim);
                    outline_ui(app, ui, &t);
                }
                PanelMode::Todos => {
                    header(ui, "TODO", t.panel, t.text_dim);
                    crate::todopanel::ui(app, ui, &t);
                }
            }
        });
}

/// v1.9: the Function List panel — every definition found in the active
/// document (outline::scan), click to jump.
fn outline_ui(app: &mut App, ui: &mut egui::Ui, t: &Theme) {
    if app.active.is_none() {
        ui.label(egui::RichText::new("Open a file to see its outline.")
            .size(11.5).color(t.text_dim));
        return;
    }
    if app.outline_items.is_empty() {
        ui.label(egui::RichText::new("No functions/classes found for this file type.")
            .size(11.5).color(t.text_dim));
        return;
    }
    let items = app.outline_items.clone();
    let cur_line = app.cursor_line_no.unwrap_or(usize::MAX);
    egui::ScrollArea::vertical()
        .id_salt("rea_outline")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for sym in &items {
                let (glyph, col) = match sym.kind {
                    crate::outline::Kind::Fn => ("ƒ", t.accent),
                    crate::outline::Kind::Class => ("⬡", t.ok),
                    crate::outline::Kind::Heading => ("§", t.text_dim),
                };
                let here = sym.line == cur_line;
                let fr = egui::Frame::default()
                    .fill(if here { t.active_row } else { Color32::TRANSPARENT })
                    .inner_margin(egui::Margin::symmetric(8.0, 3.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(glyph).size(11.0).color(col));
                            ui.label(egui::RichText::new(&sym.name).size(12.0).color(t.text));
                            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                                ui.label(egui::RichText::new(format!("{}", sym.line + 1))
                                    .size(10.0).color(t.text_faint));
                            });
                        });
                    });
                let resp = fr.response.interact(Sense::click());
                if resp.clicked() {
                    let i = app.active.unwrap();
                    app.goto_doc_line(i, sym.line, 0);
                }
                resp.on_hover_cursor(egui::CursorIcon::PointingHand);
            }
        });
}

fn header(ui: &mut egui::Ui, title: &str, fill: Color32, col: Color32) {
    egui::Frame::default()
        .fill(fill)
        .inner_margin(egui::Margin::symmetric(10.0, 8.0))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(title).size(11.0).strong().color(col));
        });
}

// ---------------------------------------------------------------- explorer

fn explorer_ui(app: &mut App, ui: &mut egui::Ui, t: &Theme) {
    ui.horizontal(|ui| {
        ui.add_space(6.0);
        if ui
            .add(egui::Button::new(
                egui::RichText::new("Open Folder").size(12.0),
            ))
            .clicked()
        {
            app.open_folder_dialog();
        }
        if ui
            .add(egui::Button::new(egui::RichText::new("Refresh").size(12.0)))
            .clicked()
        {
            app.refresh_tree();
        }
    });
    ui.horizontal(|ui| {
        ui.add_space(6.0);
        // v1.5 (#13): file operations on the workspace root
        if ui
            .add_enabled(
                app.workspace.is_some(),
                egui::Button::new(egui::RichText::new("+ File").size(12.0)),
            )
            .clicked()
        {
            let dir = app.workspace.clone().unwrap_or_default();
            app.new_file_in(dir);
        }
        if ui
            .add_enabled(
                app.workspace.is_some(),
                egui::Button::new(egui::RichText::new("+ Folder").size(12.0)),
            )
            .clicked()
        {
            let dir = app.workspace.clone().unwrap_or_default();
            app.new_folder_in(dir);
        }
    });
    // dotfiles toggle (#14) — one clean row with a real label
    ui.horizontal(|ui| {
        ui.add_space(6.0);
        let mut show = app.settings.show_dotfiles;
        if ui.checkbox(&mut show, "Show hidden files").changed() {
            app.settings.show_dotfiles = show;
            app.save_settings();
            app.refresh_tree();
        }
    });
    ui.add_space(4.0);
    ui.separator();

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            match app.workspace.clone() {
                Some(ws) => {
                    let name = ws
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| ws.display().to_string());
                    // root row
                    row_dir(app, ui, &t, &name, &ws, 0, true);
                    if !app.collapsed.contains(&ws) {
                        render_dir(app, ui, t, &ws, 1);
                    }
                }
                None => {
                    ui.add_space(10.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new("No folder open")
                                .size(12.0)
                                .color(t.text_faint),
                        );
                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new("Use Open Folder or\nthe welcome screen")
                                .size(11.0)
                                .color(t.text_faint),
                        );
                    });
                }
            }
        });
}

fn render_dir(app: &mut App, ui: &mut egui::Ui, t: &Theme, dir: &std::path::Path, depth: usize) {
    if depth > 6 {
        return;
    }
    let nodes = app.dir_list(dir);
    for n in nodes {
        if n.is_dir {
            let name = n.name.clone();
            let open_menu = row_dir(app, ui, t, &name, &n.path, depth, false);
            if !app.collapsed.contains(&n.path) {
                render_dir(app, ui, t, &n.path, depth + 1);
            }
            if open_menu.new_file {
                app.new_file_in(n.path.clone());
            }
            if open_menu.new_folder {
                app.new_folder_in(n.path.clone());
            }
            if open_menu.rename {
                app.rename_path(n.path.clone());
            }
            if open_menu.delete {
                app.delete_path(n.path.clone());
            }
        } else {
            let is_active = app
                .docs
                .get(app.active.unwrap_or(usize::MAX))
                .and_then(|d| d.path.clone())
                .map(|p| p == n.path)
                .unwrap_or(false);
            let fr = egui::Frame::default()
                .fill(if is_active { t.active_row } else { Color32::TRANSPARENT })
                .rounding(Rounding::same(4.0))
                .inner_margin(egui::Margin::symmetric(4.0, 2.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_space((depth * 12) as f32);
                        icons::badge(ui, &n.ext);
                        let mut txt = egui::RichText::new(n.name.clone()).size(13.0);
                        if is_active {
                            txt = txt.color(t.text);
                        }
                        ui.label(txt);
                    });
                });
            let resp = fr.response.interact(Sense::click());
            if resp.clicked() {
                app.open_path(&n.path);
            }
            // v1.5 (#13): right-click menu with the file operations
            let mut act = RowMenu::default();
            resp.context_menu(|ui| {
                if ui.button("Open").clicked() {
                    app.open_path(&n.path.clone());
                    ui.close_menu();
                }
                if ui.button("Rename…").clicked() {
                    act.rename = true;
                    ui.close_menu();
                }
                if ui.button("Delete…").clicked() {
                    act.delete = true;
                    ui.close_menu();
                }
                if ui.button("New File Here…").clicked() {
                    act.new_file = true;
                    ui.close_menu();
                }
            });
            if act.rename {
                app.rename_path(n.path.clone());
            }
            if act.delete {
                app.delete_path(n.path.clone());
            }
            if act.new_file {
                let dir = n.path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
                app.new_file_in(dir);
            }
            resp.on_hover_cursor(egui::CursorIcon::PointingHand);
        }
    }
}

/// actions picked from an explorer context menu
#[derive(Default)]
struct RowMenu {
    new_file: bool,
    new_folder: bool,
    rename: bool,
    delete: bool,
}

fn row_dir(
    app: &mut App,
    ui: &mut egui::Ui,
    t: &Theme,
    name: &str,
    path: &std::path::Path,
    depth: usize,
    root: bool,
) -> RowMenu {
    let collapsed = app.collapsed.contains(path);
    let fr = egui::Frame::default()
        .fill(Color32::TRANSPARENT)
        .rounding(Rounding::same(4.0))
        .inner_margin(egui::Margin::symmetric(4.0, 2.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add_space((depth * 12) as f32);
                let (cpos, _) = ui.allocate_exact_size(vec2(12.0, 16.0), Sense::hover());
                icons::chevron(
                    &ui.painter(),
                    cpos.center(),
                    !collapsed,
                    t.text_dim,
                );
                icons::folder(
                    &ui.painter(),
                    cpos.center() + vec2(11.0, 0.0),
                    t.warn,
                );
                let mut txt = egui::RichText::new(name).size(13.0).strong();
                if root {
                    txt = txt.color(t.accent);
                }
                ui.label(txt);
            });
        });
    let resp = fr.response.interact(Sense::click());
    if resp.clicked() {
        if collapsed {
            app.collapsed.remove(path);
        } else {
            app.collapsed.insert(path.to_path_buf());
        }
    }
    // v1.5 (#13): right-click menu on folders
    let mut act = RowMenu::default();
    resp.context_menu(|ui| {
        if ui.button("New File…").clicked() {
            act.new_file = true;
            ui.close_menu();
        }
        if ui.button("New Folder…").clicked() {
            act.new_folder = true;
            ui.close_menu();
        }
        if !root {
            ui.separator();
            if ui.button("Rename…").clicked() {
                act.rename = true;
                ui.close_menu();
            }
            if ui.button("Delete…").clicked() {
                act.delete = true;
                ui.close_menu();
            }
        }
    });
    resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    act
}

// ---------------------------------------------------------------- search panel (Go to File + full-text)

fn search_ui(app: &mut App, ui: &mut egui::Ui, t: &Theme) {
    ui.add_space(4.0);

    // tab switch: [Go to File] [Full-Text]
    ui.horizontal(|ui| {
        if ui
            .selectable_label(app.search_tab_files, "Go to File")
            .clicked()
        {
            app.search_tab_files = true;
        }
        if ui
            .selectable_label(!app.search_tab_files, "Full-Text")
            .clicked()
        {
            app.search_tab_files = false;
            app.refresh_tree();
        }
    });
    ui.separator();

    if app.search_tab_files {
        go_to_file_ui(app, ui, t);
    } else {
        crate::wssearch::show(app, ui, t);
    }
}

fn go_to_file_ui(app: &mut App, ui: &mut egui::Ui, t: &Theme) {
    let mut q = app.search_query.clone();
    let resp = ui.add(
        egui::TextEdit::singleline(&mut q)
            .hint_text("Filter files…")
            .desired_width(ui.available_width() - 16.0),
    );
    if resp.changed() {
        app.search_query = q;
    }
    ui.add_space(4.0);

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let ql = app.search_query.to_lowercase();
            let files: Vec<std::path::PathBuf> = if app.workspace.is_some() {
                app.ws_files.clone()
            } else {
                Vec::new()
            };
            let mut shown = 0usize;
            for p in files {
                if shown >= 200 {
                    break;
                }
                let name = p
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                if !ql.is_empty() && !name.to_lowercase().contains(&ql) {
                    continue;
                }
                shown += 1;
                let ext = p
                    .extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                let fr = egui::Frame::default()
                    .fill(Color32::TRANSPARENT)
                    .rounding(Rounding::same(4.0))
                    .inner_margin(egui::Margin::symmetric(6.0, 3.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.add_space(4.0);
                            icons::badge(ui, &ext);
                            ui.label(egui::RichText::new(&name).size(13.0).color(t.text));
                        });
                    });
                let r = fr.response.interact(Sense::click());
                if r.clicked() {
                    app.open_path(&p);
                }
                r.on_hover_cursor(egui::CursorIcon::PointingHand);
            }
            if shown == 0 {
                ui.label(
                    egui::RichText::new("No files. Open a folder first.")
                        .size(12.0)
                        .color(t.text_faint),
                );
            }
        });
}

// ---------------------------------------------------------------- status bar

pub fn show_status(app: &mut App, ctx: &egui::Context) {
    let t = app.theme_now.clone();
    egui::TopBottomPanel::bottom("rea_status")
        .exact_height(26.0)
        .frame(egui::Frame::none().fill(t.accent))
        .show(ctx, |ui| {
            ui.with_layout(egui::Layout::left_to_right(Align::Center), |ui| {
                ui.add_space(8.0);

                let (name, ext) = match app.active {
                    Some(i) => {
                        let d = &app.docs[i];
                        (d.name.clone(), d.ext())
                    }
                    None => ("Ready".into(), String::new()),
                };
                if !ext.is_empty() {
                    // accent-on-accent badge: draw with white chip
                    let (lbl, col, light) = icons::ext_meta(&ext);
                    let (r, _) =
                        ui.allocate_exact_size(vec2(20.0, 15.0), Sense::hover());
                    ui.painter().rect_filled(r, 4.0, col);
                    let tcol = if light {
                        Color32::from_rgb(35, 35, 35)
                    } else {
                        Color32::WHITE
                    };
                    ui.painter().text(
                        r.center(),
                        Align2::CENTER_CENTER,
                        lbl,
                        egui::FontId::proportional(8.0),
                        tcol,
                    );
                }
                ui.label(
                    egui::RichText::new(name)
                        .size(11.5)
                        .color(t.accent_text),
                );

                // v1.5: show the logical LINE (wrap-correct), not the row
                if let (Some((_, colu)), Some(ln)) = (app.cursor, app.cursor_line_no) {
                    ui.label(
                        egui::RichText::new(format!("Ln {}, Col {}", ln + 1, colu + 1))
                            .size(11.0)
                            .color(t.accent_text.gamma_multiply(0.85)),
                    );
                }
                // v1.9: zoom indicator (only when not 100%)
                // v2.0: percent now reflects the REAL rendered font scale
                // (editor_font_size / setting), so when the 9px floor clamps
                // deep zoom-out the number matches what you see.
                if (app.zoom - 1.0).abs() > f32::EPSILON {
                    let pct = if app.settings.font_size > 0.0 {
                        (app.editor_font_size() / app.settings.font_size * 100.0).round() as i32
                    } else {
                        (app.zoom * 100.0).round() as i32
                    };
                    ui.label(
                        egui::RichText::new(format!("{}%", pct))
                            .size(11.0)
                            .color(t.accent_text),
                    );
                }
                // v2.0: macro recording + column-mode indicators
                if app.macro_rec.is_some() {
                    ui.label(
                        egui::RichText::new(format!("● REC {}", app.macro_rec.as_ref().map(|v| v.len()).unwrap_or(0)))
                            .size(11.0)
                            .color(t.err),
                    );
                }
                if app.colsel.is_some() {
                    ui.label(
                        egui::RichText::new("COLUMN")
                            .size(11.0)
                            .color(t.accent),
                    );
                }

                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    // v1.6: Live Server pill (start/stop) — rightmost
                    crate::liveserver::pill_ui(app, ui, &t, t.accent_text);
                    let st = app.status_text();
                    if !st.is_empty() {
                        ui.label(
                            egui::RichText::new(st)
                                .size(11.0)
                                .color(t.accent_text),
                        );
                    }
                    let term_label = if !app.terms.is_empty() {
                        let i = app.term_active.min(app.terms.len() - 1);
                        format!(
                            "{} ×{}",
                            app.terms[i].kind.label(),
                            app.terms.len()
                        )
                    } else {
                        "UTF-8".to_string()
                    };
                    ui.label(
                        egui::RichText::new(term_label)
                            .size(11.0)
                            .color(t.accent_text.gamma_multiply(0.85)),
                    );
                    let lang = crate::highlight::Highlighter::lang_name(
                        &app.active
                            .map(|i| app.docs[i].ext())
                            .unwrap_or_default(),
                    );
                    ui.label(
                        egui::RichText::new(lang)
                            .size(11.0)
                            .color(t.accent_text.gamma_multiply(0.85)),
                    );
                    // v2.1: EOL indicator (LF / CRLF)
                    let eol_lbl = match app.active {
                        Some(i) => {
                            if app.docs[i].crlf {
                                "CRLF"
                            } else {
                                "LF"
                            }
                        }
                        None => "",
                    };
                    if !eol_lbl.is_empty() {
                        ui.label(
                            egui::RichText::new(eol_lbl)
                                .size(11.0)
                                .color(t.accent_text.gamma_multiply(0.85)),
                        );
                    }
                    ui.add_space(8.0);
                });
            });
        });
}

// ---------------------------------------------------------------- about window

pub fn show_about(app: &mut App, ctx: &egui::Context) {
    if !app.about_open {
        return;
    }
    let t = app.theme_now.clone();
    egui::Window::new(
        egui::RichText::new("About REA Studio")
            .size(15.0)
            .strong()
            .color(t.text),
    )
    .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
    .collapsible(false)
    .resizable(false)
    .show(ctx, |ui| {
        ui.set_width(340.0);
        ui.vertical_centered(|ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add_space(ui.available_width() * 0.5 - 62.0);
                ui.label(
                    egui::RichText::new("REA ")
                        .size(30.0)
                        .strong()
                        .color(t.accent),
                );
                ui.label(
                    egui::RichText::new("Studio")
                        .size(30.0)
                        .strong()
                        .color(t.text),
                );
            });
            ui.label(
                egui::RichText::new(concat!("Version ", env!("CARGO_PKG_VERSION")))
                    .size(12.0)
                    .color(t.text_dim),
            );
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new("A fast, lightweight code editor.\nBuilt with 100% pure Rust (eframe / egui).\nBased on the idea of morluto/rea.")
                    .size(12.0)
                    .color(t.text_dim),
            );
            ui.add_space(10.0);
            if ui
                .add(egui::Button::new(
                    egui::RichText::new("Close").size(13.0),
                ))
                .clicked()
            {
                app.about_open = false;
            }
            ui.add_space(4.0);
        });
    });
}

// silence unused warnings for helpers kept for future polish
#[allow(dead_code)]
fn _unused(_v: Vec2) {}

// ---------------------------------------------------------------- v1.5 modals

/// Unsaved-changes confirmations (#12) + the generic input dialog used for
/// conditional breakpoints, new file/folder, rename and delete (#13, #16).
pub fn show_modals(app: &mut App, ctx: &egui::Context, t: &Theme) {
    // ---- close-tab confirmation ----
    if let Some(i) = app.pending_close {
        if i >= app.docs.len() {
            app.pending_close = None;
        } else {
            let name = app.docs[i].name.clone();
            let mut act: u8 = 0; // 1=save 2=discard 3=cancel
            egui::Window::new(
                egui::RichText::new("Unsaved changes")
                    .size(15.0)
                    .strong()
                    .color(t.text),
            )
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.set_width(360.0);
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(format!("“{name}” has unsaved changes."))
                        .size(13.0)
                        .color(t.text),
                );
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new(egui::RichText::new("Save").strong())).clicked() {
                        act = 1;
                    }
                    if ui.add(egui::Button::new(egui::RichText::new("Don't Save"))).clicked() {
                        act = 2;
                    }
                    if ui.add(egui::Button::new(egui::RichText::new("Cancel"))).clicked() {
                        act = 3;
                    }
                });
            });
            match act {
                1 => {
                    app.pending_close = None;
                    app.active = Some(i);
                    app.save();
                    // if the save dialog was cancelled the tab stays open
                    if !app.docs[i].dirty {
                        app.close_doc(i);
                    }
                }
                2 => {
                    let j = i;
                    app.pending_close = None;
                    app.docs[j].dirty = false;
                    app.close_doc(j);
                }
                3 => app.pending_close = None,
                _ => {}
            }
            return;
        }
    }

    // ---- exit confirmation ----
    if app.exit_confirm {
        let dirty_count = app.docs.iter().filter(|d| d.dirty).count();
        let mut act: u8 = 0; // 1=save+exit 2=exit-without-save 3=cancel
        egui::Window::new(
            egui::RichText::new("Exit REA Studio")
                .size(15.0)
                .strong()
                .color(t.text),
        )
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.set_width(380.0);
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(format!(
                    "{dirty_count} file(s) have unsaved changes. Save before exiting?"
                ))
                .size(13.0)
                .color(t.text),
            );
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.add(egui::Button::new(egui::RichText::new("Save & Exit").strong())).clicked() {
                    act = 1;
                }
                if ui.add(egui::Button::new(egui::RichText::new("Exit without Saving"))).clicked() {
                    act = 2;
                }
                if ui.add(egui::Button::new(egui::RichText::new("Cancel"))).clicked() {
                    act = 3;
                }
            });
        });
        match act {
            1 => {
                app.exit_confirm = false;
                app.save_all();
                app.exit_allow = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            2 => {
                app.exit_confirm = false;
                app.exit_allow = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            3 => app.exit_confirm = false,
            _ => {}
        }
        return;
    }

    // ---- generic input dialog (conditional bp / new file / rename / delete) ----
    if let Some(dlg) = app.input_dialog.clone() {
        let is_delete = dlg.action.starts_with("delete:");
        let mut close = false;
        let mut confirm = false;
        egui::Window::new(
            egui::RichText::new(&dlg.title).size(15.0).strong().color(t.text),
        )
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.set_width(400.0);
            ui.add_space(4.0);
            ui.label(egui::RichText::new(&dlg.label).size(12.0).color(t.text_dim));
            ui.add_space(6.0);
            let mut v = dlg.value.clone();
            let resp = ui.add(
                egui::TextEdit::singleline(&mut v)
                    .desired_width(ui.available_width())
                    .font(egui::TextStyle::Monospace),
            );
            if resp.changed() {
                app.input_dialog.as_mut().unwrap().value = v.clone();
            }
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                confirm = true;
            }
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
            if is_delete && resp.has_focus() {
                // hint that the name must match exactly
                ui.label(
                    egui::RichText::new("type the exact name to enable deletion")
                        .size(10.5)
                        .color(t.text_faint),
                );
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let label = if is_delete { "Delete" } else { "OK" };
                if ui.add(egui::Button::new(egui::RichText::new(label).strong())).clicked() {
                    confirm = true;
                }
                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
        });
        if confirm {
            app.confirm_input_dialog();
        } else if close {
            app.input_dialog = None;
        }
    }
}
