use eframe::egui;
use egui::{Align, Align2, Color32, FontId, Rounding, Sense, Stroke, TextStyle, pos2, vec2};

use crate::app::{App, Doc};
use crate::icons;

// ---------------------------------------------------------------- documents

impl Doc {
    pub fn ext(&self) -> String {
        self.path
            .as_ref()
            .map(|p| {
                p.extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .unwrap_or_default()
            })
            .unwrap_or_default()
    }
}

// ---------------------------------------------------------------- main entry

pub fn show(app: &mut App, ctx: &egui::Context) {
    let t = app.theme_now.clone();

    // ---- v1.7: split editor pane (SidePanel must precede the CentralPanel) ----
    if app.split_on && app.split_doc.is_some() && app.docs.len() > 1 {
        egui::SidePanel::right("rea_split")
            .resizable(true)
            .default_width((ctx.screen_rect().width() * 0.38).clamp(280.0, 640.0))
            .width_range(200.0..=f32::INFINITY)
            .frame(egui::Frame::none().fill(t.editor_bg))
            .show(ctx, |ui| {
                split_pane(app, ui, &t);
            });
    }

    egui::CentralPanel::default()
        .frame(egui::Frame::none().fill(t.editor_bg))
        .show(ctx, |ui| {
            app.editor_rect = ui.max_rect();
            if app.docs.is_empty() || app.active.is_none() {
                welcome(app, ui, &t);
            } else {
                file_editor(app, ui, &t, "");
            }
        });

    if app.find.open {
        find_overlay(app, ctx, &t);
    }
    if app.suggest.is_some() {
        suggest_popup(app, ctx, &t);
    }
}

// ---------------------------------------------------------------- v1.7 split pane

/// Right-side split pane: a second open document with its own cursor,
/// fully editable (state-swap around file_editor). suffix "_split" keeps
/// egui ids apart from the main pane.
fn split_pane(app: &mut App, ui: &mut egui::Ui, t: &crate::theme::Theme) {
    let Some(si) = app.split_doc else { return };
    if si >= app.docs.len() {
        app.split_doc = None;
        return;
    }

    // ---- header: file picker + close ----
    egui::Frame::default()
        .fill(t.panel2)
        .inner_margin(egui::Margin::symmetric(8.0, 3.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("SPLIT")
                        .size(9.5)
                        .strong()
                        .color(t.text_dim),
                );
                let names: Vec<String> = app
                    .docs
                    .iter()
                    .map(|d| d.name.clone())
                    .collect();
                let cur = names[si].clone();
                egui::ComboBox::from_id_source("rea_split_sel")
                    .width(ui.available_width() - 40.0)
                    .selected_text(cur)
                    .show_ui(ui, |ui| {
                        for (i, n) in names.iter().enumerate() {
                            if i != app.active.unwrap_or(usize::MAX) {
                                ui.selectable_value(&mut app.split_doc, Some(i), n);
                            } else {
                                ui.add_enabled(false, egui::Button::new(n.as_str()));
                            }
                        }
                    });
                if ui.button(egui::RichText::new("✕").size(10.0)).clicked() {
                    app.split_on = false;
                    app.split_doc = None;
                }
            });
        });
    ui.separator();

    // ---- swap editor state: render the split doc through file_editor ----
    let saved_active = app.active;
    let saved_cursor = app.cursor;
    let saved_line_no = app.cursor_line_no;
    let saved_sel = app.sel_range;
    let saved_suggest = app.suggest.take();
    let saved_suggest_pos = app.suggest_pos;
    let saved_editor_focused = app.editor_focused;
    let saved_multi = app.multi.take();
    let saved_pending_scroll = app.pending_scroll.take();
    let saved_suppress = app.suppress_len;
    let saved_cursor_char = app.cursor_char;

    app.active = Some(si);
    app.cursor = app.split_cursor;
    app.cursor_line_no = None;
    app.sel_range = app.split_sel;
    app.cursor_char = None;
    app.suppress_len = None;

    file_editor(app, ui, t, "_split");

    app.split_cursor = app.cursor;
    app.split_sel = app.sel_range;

    app.active = saved_active;
    app.cursor = saved_cursor;
    app.cursor_line_no = saved_line_no;
    app.sel_range = saved_sel;
    app.suggest = saved_suggest;
    app.suggest_pos = saved_suggest_pos;
    app.editor_focused = saved_editor_focused;
    app.multi = saved_multi;
    app.pending_scroll = saved_pending_scroll;
    app.suppress_len = saved_suppress;
    app.cursor_char = saved_cursor_char;
}

// ---------------------------------------------------------------- welcome page

fn welcome(app: &mut App, ui: &mut egui::Ui, t: &crate::theme::Theme) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.12);

        ui.horizontal(|ui| {
            ui.add_space(ui.available_width() * 0.5 - 130.0);
            ui.label(
                egui::RichText::new("REA ")
                    .size(46.0)
                    .strong()
                    .color(t.accent),
            );
            ui.label(egui::RichText::new("Studio").size(46.0).strong().color(t.text));
        });
        ui.label(
            egui::RichText::new("A fast, lightweight code editor — 100% Rust")
                .size(14.0)
                .color(t.text_dim),
        );
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(concat!("v", env!("CARGO_PKG_VERSION")))
                .size(11.0)
                .color(t.text_faint),
        );

        ui.add_space(34.0);

        // four soft-shadow action buttons
        ui.horizontal(|ui| {
            let w = 128.0_f32;
            let total = w * 4.0 + 12.0 * 3.0;
            ui.add_space(ui.available_width() * 0.5 - total * 0.5);
            if shadow_button(ui, "New File", Act::File, t, w) {
                app.new_file();
            }
            ui.add_space(12.0);
            if shadow_button(ui, "Open File", Act::Open, t, w) {
                app.open_file_dialog();
            }
            ui.add_space(12.0);
            if shadow_button(ui, "Open Folder", Act::Folder, t, w) {
                app.open_folder_dialog();
            }
            ui.add_space(12.0);
            if shadow_button(ui, "Clone Repo", Act::GitHub, t, w) {
                app.open_github_dialog();
            }
        });

        ui.add_space(38.0);

        // recent folders (v1.4)
        if !app.recent_folders.is_empty() {
            ui.label(
                egui::RichText::new("RECENT FOLDERS")
                    .size(11.0)
                    .strong()
                    .color(t.text_faint),
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let folders: Vec<_> = app.recent_folders.iter().take(4).cloned().collect();
                for path in folders {
                    let name = path
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| path.display().to_string());
                    let b = egui::Button::new(
                        egui::RichText::new(format!("📁 {name}")).size(12.0).color(t.text),
                    );
                    if ui.add(b).clicked() {
                        app.set_workspace(path.clone());
                    }
                }
            });
            ui.add_space(16.0);
        }

        if !app.recents.is_empty() {
            ui.label(
                egui::RichText::new("RECENT FILES")
                    .size(11.0)
                    .strong()
                    .color(t.text_faint),
            );
            ui.add_space(8.0);

            let max_rows = 6;
            let recents: Vec<_> = app.recents.iter().take(max_rows).cloned().collect();
            egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
                for path in recents {
                    let name = path
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| path.display().to_string());
                    let dir = path
                        .parent()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default();
                    let ext = path
                        .extension()
                        .map(|e| e.to_string_lossy().to_lowercase())
                        .unwrap_or_default();

                    let fr = egui::Frame::default()
                        .fill(t.bg.gamma_multiply(0.0))
                        .rounding(Rounding::same(6.0))
                        .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                icons::badge(ui, &ext);
                                ui.label(egui::RichText::new(&name).size(13.0).color(t.text));
                                ui.label(
                                    egui::RichText::new(&dir).size(11.0).color(t.text_faint),
                                );
                            });
                        });
                    let resp = fr.response.interact(Sense::click());
                    if resp.clicked() {
                        app.open_path(&path);
                    }
                    resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                }
            });
        }
    });
}

enum Act {
    File,
    Open,
    Folder,
    GitHub,
}

/// A button wrapped in a rounded Frame with a soft shadow (the REA look).
fn shadow_button(ui: &mut egui::Ui, label: &str, icon: Act, t: &crate::theme::Theme, w: f32) -> bool {
    let resp = egui::Frame::default()
        .fill(t.bg)
        .stroke(Stroke::new(1.0, t.border))
        .rounding(Rounding::same(10.0))
        .inner_margin(egui::Margin::symmetric(0.0, 14.0))
        .shadow(egui::Shadow {
            offset: vec2(0.0, 4.0),
            blur: 14.0,
            spread: 0.0,
            color: t.shadow_col,
        })
        .show(ui, |ui| {
            ui.set_width(w);
            ui.vertical_centered(|ui| {
                let (pos, _) = ui.allocate_exact_size(vec2(w, 34.0), Sense::hover());
                let r = pos;
                let p = ui.painter_at(r);
                let c = r.center();
                let sw = Stroke::new(1.6, t.accent);
                match icon {
                    Act::File => {
                        p.rect_stroke(
                            egui::Rect::from_min_size(c + vec2(-7.0, -10.0), vec2(14.0, 20.0)),
                            2.0,
                            sw,
                        );
                        p.line_segment([c + vec2(-4.0, 0.0), c + vec2(4.0, 0.0)], sw);
                    }
                    Act::Open => {
                        p.circle_stroke(c + vec2(-1.0, -1.0), 6.0, sw);
                        p.line_segment([c + vec2(3.0, 3.0), c + vec2(8.0, 8.0)], sw);
                    }
                    Act::Folder => {
                        icons::folder(&p, c, t.accent);
                    }
                    Act::GitHub => {
                        // git branch glyph: two nodes + a forked line
                        p.circle_stroke(c + vec2(-5.0, -6.0), 3.0, sw);
                        p.circle_stroke(c + vec2(-5.0, 6.0), 3.0, sw);
                        p.circle_stroke(c + vec2(6.0, -1.0), 3.0, sw);
                        p.line_segment([c + vec2(-5.0, -3.0), c + vec2(-5.0, 3.0)], sw);
                        p.line_segment([c + vec2(-5.0, 0.0), c + vec2(3.0, -1.0)], sw);
                    }
                }
                ui.add_space(6.0);
                ui.label(egui::RichText::new(label).size(14.0).color(t.text));
            });
        })
        .response
        .interact(Sense::click());
    resp.clicked()
}

// ---------------------------------------------------------------- file editor

fn file_editor(app: &mut App, ui: &mut egui::Ui, t: &crate::theme::Theme, suffix: &str) {
    let ai = app.active.unwrap();
    let gutter_w = 56.0;
    // v1.9: settings.font_size × zoom (Ctrl+wheel / Ctrl+= / Ctrl+-) — also
    // fixes the old bug where the font-size setting never reached the editor
    // text (the layouter hard-coded 14px).
    //
    // v2.0 ZOOM-OUT FIX: the old code had `.max(10.0)` on the font size and
    // `.max(18.0)` on the row height. The galley the TextEdit actually renders
    // uses the REAL row height of the font (≈13px at 10px), while the gutter,
    // line numbers, bookmarks, current-line highlight and scroll math used the
    // clamped 18px — so everything below ~76% zoom drifted out of alignment.
    // Both clamps are gone; alignment is now exact at every zoom level.
    let mono_size = app.editor_font_size();
    let row_h = ui
        .fonts(|f| f.row_height(&FontId::monospace(mono_size)));

    // copy what the layouter needs (avoids borrow conflicts)
    let ext = app.docs[ai].ext();
    let dark = t.dark;
    let word_wrap = app.settings.word_wrap;

    // ---- conflict / recovered banners (file watcher + crash recovery) ----
    if app.docs[ai].conflict.is_some() {
        let mut reload = false;
        let mut keep = false;
        egui::Frame::default()
            .fill(t.warn.gamma_multiply(0.18))
            .inner_margin(egui::Margin::symmetric(10.0, 6.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "⚠  {} changed on disk.",
                            app.docs[ai].name
                        ))
                        .size(12.0)
                        .color(t.warn),
                    );
                    if ui.button("Reload from disk").clicked() {
                        reload = true;
                    }
                    if ui.button("Keep my version").clicked() {
                        keep = true;
                    }
                });
            });
        if reload {
            app.doc_reload_from_disk();
        }
        if keep {
            app.doc_keep_mine();
        }
    }
    if app.docs[ai].recovered.is_some() && app.docs[ai].conflict.is_none() {
        let mut restore = false;
        let mut discard = false;
        egui::Frame::default()
            .fill(t.ok.gamma_multiply(0.15))
            .inner_margin(egui::Margin::symmetric(10.0, 6.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Unsaved changes from the last session were recovered.")
                            .size(12.0)
                            .color(t.ok),
                    );
                    if ui.button("Restore").clicked() {
                        restore = true;
                    }
                    if ui.button("Discard").clicked() {
                        discard = true;
                    }
                });
            });
        if restore {
            app.restore_recovered();
        }
        if discard {
            app.discard_recovered();
        }
    }

    // ---- breadcrumbs bar ----
    if app.settings.breadcrumbs {
        egui::Frame::default()
            .fill(t.editor_bg)
            .inner_margin(egui::Margin::symmetric(10.0, 3.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let mut crumbs: Vec<String> = Vec::new();
                    if let (Some(ws), Some(dp)) = (&app.workspace, &app.docs[ai].path) {
                        if let Ok(rel) = dp.strip_prefix(ws) {
                            for c in rel.iter() {
                                crumbs.push(c.to_string_lossy().to_string());
                            }
                        }
                        if crumbs.is_empty() {
                            crumbs.push(dp.display().to_string());
                        }
                    } else {
                        crumbs.push(app.docs[ai].name.clone());
                    }
                    let symbol = app.cursor.and_then(|(ln, _)| {
                        // v1.5 (#19): walk UP from the cursor line to the nearest
                        // definition — the symbol shows for the WHOLE function
                        // body, not just on the line where it is declared.
                        // (Old code: only `nth(ln)` — the cursor line itself.)
                        let text = &app.docs[ai].text;
                        let start = ln.min(text.matches('\n').count());
                        (0..=start)
                            .rev()
                            .take(2000)
                            .find_map(|l| {
                                text.split('\n').nth(l).and_then(crate::editops::line_symbol)
                            })
                    });
                    let n = crumbs.len();
                    for (idx, c) in crumbs.iter().enumerate() {
                        let last = idx + 1 == n;
                        let col = if last { t.text } else { t.text_faint };
                        ui.label(egui::RichText::new(c.as_str()).size(11.0).color(col));
                        if !last {
                            ui.label(egui::RichText::new("›").size(11.0).color(t.text_faint));
                        }
                    }
                    if let Some(sym) = symbol {
                        ui.label(egui::RichText::new("›").size(11.0).color(t.text_faint));
                        ui.label(egui::RichText::new(sym).size(11.0).color(t.accent));
                    }
                });
            });
        ui.separator();
    }

    // clip = everything below the banners / breadcrumbs
    let full_rect = ui.max_rect();
    let content_top = ui.cursor().top().max(full_rect.top());
    let clip = egui::Rect::from_min_max(pos2(full_rect.left(), content_top), full_rect.max);

    // gutter background
    ui.painter().rect_filled(
        egui::Rect::from_min_size(clip.min, vec2(gutter_w, clip.height())),
        0.0,
        t.gutter_bg,
    );

    let mut cursor_rc: Option<(usize, usize)> = None;
    let mut galley_top = clip.top();
    // code-suggestion tracking
    let mut editor_focus = false;
    let mut editor_edited = false;
    let mut cursor_char_idx: Option<usize> = None;
    let mut anchor_pos: Option<egui::Pos2> = None;
    // post-edit fixups + undo (v1.4)
    let mut typed_char: Option<char> = None;
    let mut enter_pressed = false;
    let mut galley_ref: Option<std::sync::Arc<egui::Galley>> = None;
    let mut galley_pos_out = egui::Pos2::ZERO;
    // v1.5: (is_alt_click, is_shift, pointer pos) captured from the editor
    // response (v2.0: shift added for column anchoring)
    let mut click_info: Option<(bool, bool, Option<egui::Pos2>)> = None;

    egui::ScrollArea::both()
        .id_salt(format!("editor_{ai}{suffix}"))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let hl = &app.hl;
            let doc = &mut app.docs[ai];
            let text_len_lines = 1 + doc.text.matches('\n').count();

            let mut layouter =
                |ui: &egui::Ui, text: &str, wrap_width: f32| -> std::sync::Arc<egui::Galley> {
                    let mut job = hl.job(text, wrap_width, dark, &ext, mono_size);
                    // word-wrap toggle (Alt+Z / Settings) — INFINITY = no wrap
                    job.wrap.max_width = if word_wrap { wrap_width } else { f32::INFINITY };
                    ui.fonts(|f| f.layout_job(job))
                };

            egui::Frame::none()
                .inner_margin(egui::Margin {
                    left: gutter_w,
                    right: 8.0,
                    top: 6.0,
                    bottom: 40.0,
                })
                .show(ui, |ui| {
                    let out = egui::TextEdit::multiline(&mut doc.text)
                        .id(egui::Id::new(format!("rea_editor_{ai}{suffix}")))
                        .font(TextStyle::Monospace)
                        .desired_rows(1)
                        .frame(false)
                        .desired_width(f32::INFINITY)
                        .layouter(&mut layouter)
                        .show(ui);

                    // v1.5 multi-cursor interactions on the editor surface:
                    // click info is CAPTURED here and processed after the
                    // ScrollArea closure ends (borrow checker: the layouter
                    // holds &app.hl, so app can't be borrowed mutably inside)
                    // v2.0: shift flag added — Alt+Shift+Click anchors the
                    // column selection (plain Alt+Click adds a cursor).
                    if out.response.clicked() {
                        let (alt, sh) = ui.input(|i| (i.modifiers.alt, i.modifiers.shift));
                        click_info = Some((alt, sh, out.response.interact_pointer_pos()));
                    }

                    if let Some(cr) = out.cursor_range {
                        let rc = cr.primary.rcursor;
                        cursor_rc = Some((rc.row, rc.column));
                        // char-index + screen anchor for the suggestion popup
                        let cc = cr.primary.ccursor.index;
                        cursor_char_idx = Some(cc);
                        let cur = out.galley.from_ccursor(cr.primary.ccursor);
                        let prect = out.galley.pos_from_cursor(&cur);
                        anchor_pos = Some(out.galley_pos + prect.left_bottom().to_vec2());
                    }
                    editor_focus = out.response.has_focus();
                    editor_edited = out.response.changed();
                    galley_top = out.galley_pos.y;

                    // v1.4: typed chars + Enter for post-processing, galley for overlays
                    ui.input(|i| {
                        for ev in &i.events {
                            if let egui::Event::Text(tx) = ev {
                                if tx.chars().count() == 1 {
                                    typed_char = tx.chars().next();
                                }
                            }
                            if let egui::Event::Key { key: egui::Key::Enter, pressed: true, .. } = ev {
                                enter_pressed = true;
                            }
                        }
                    });
                    galley_ref = Some(out.galley.clone());
                    galley_pos_out = out.galley_pos;

                    // scroll to the active find match if pending.
                    // v1.5: resolves the line through the galley so it lands
                    // correctly even when word-wrap splits lines into rows (#11)
                    if let Some(line) = app.pending_scroll.take() {
                        let c = crate::util::line_col_to_char(&doc.text, line, 0);
                        let cur = out.galley.from_ccursor(egui::text::CCursor::new(c));
                        let pr = out.galley.pos_from_cursor(&cur);
                        let y = out.galley_pos.y + pr.top();
                        let r = egui::Rect::from_min_size(
                            pos2(out.galley_pos.x, y),
                            vec2(400.0, row_h),
                        );
                        ui.scroll_to_rect(r, Some(Align::Center));
                    }
                });

            let _ = text_len_lines;
        });

    // ---- v1.5: process the captured editor click (#2 / #6) ----
    // v2.0: Alt+Shift+Click anchors the column selection; plain click exits it.
    if let Some((alt, sh, click)) = click_info {
        if alt {
            if let (Some(click), Some(g)) = (click, &galley_ref) {
                let n = char_from_click(g, galley_pos_out, click);
                if sh {
                    app.col_anchor_at(ai, n);
                } else {
                    app.alt_click_add_cursor(ai, n);
                }
            }
        } else {
            // a plain click cancels the multi state (#2) — before this,
            // clicking left the multi regions stale and the next
            // multi_apply_typed wrote at the WRONG offsets
            app.cancel_multi();
            app.col_exit();
        }
    }

    // ---- v1.4: undo recording + post-edit fixups (outside the doc borrow) ----
    app.cursor_char = cursor_char_idx;
    if editor_focus && cursor_char_idx.is_some() {
        // capture the current selection from the TextEdit state (for wrap / Ctrl+D)
        let id = egui::Id::new(format!("rea_editor_{ai}{suffix}"));
        if let Some(st) = egui::widgets::text_edit::TextEditState::load(&app.ctx, id) {
            if let Some(range) = st.cursor.char_range() {
                let a = range.primary.index;
                let b = range.secondary.index;
                app.sel_range = Some((a.min(b), a.max(b)));
            }
        }
    } else {
        app.sel_range = None;
    }
    if editor_edited && app.multi.is_none() && !app.suppress_len.is_some() {
        // undo bookkeeping for this keystroke (burst-coalesced)
        app.notify_edit(ai, cursor_char_idx);
        // auto-pair / skip-over / auto-indent
        let entered = enter_pressed
            || (typed_char.is_none()
                && ui.input(|i| i.key_pressed(egui::Key::Enter)));
        app.post_edit_fixup(ai, typed_char, entered);
        // v1.9: keep bookmarks attached to their code when lines shift
        app.bookmark_shift_after_edit(ai);
    } else if editor_edited && app.multi.is_some() {
        // text arrived via multi_apply_typed (consumed pre-show) — nothing to do
    }

    // ---- code suggestions state update (outside the doc borrow) ----
    app.editor_focused = editor_focus;
    app.suggest_pos = if editor_focus { anchor_pos } else { None };
    if editor_focus && cursor_char_idx.is_some() {
        let cc = cursor_char_idx.unwrap();
        if let Some(l) = app.suppress_len {
            if app.docs[ai].text.len() != l {
                app.suppress_len = None;
            }
        }
        let suppressed = app.suppress_len.is_some();
        let needs = editor_edited || app.suggest.is_some();
        if needs {
            // throttle the whole-document word rescan (#18): on every keystroke
            // it used to rescan thousands of lines; 150ms keeps the popup fresh
            // without re-hashing the file for every character typed
            let cache_fresh = app
                .word_cache_time
                .map(|t| t.elapsed().as_millis() < 150)
                .unwrap_or(false)
                && app.word_cache_sig.0 == ai;
            let sig = (ai, app.docs[ai].text.chars().count());
            if !cache_fresh && app.word_cache_sig != sig {
                app.word_cache = crate::complete::doc_words(&app.docs[ai].text, 4000);
                app.word_cache_sig = sig;
                app.word_cache_time = Some(std::time::Instant::now());
            }
            let chars: Vec<char> = app.docs[ai].text.chars().collect();
            let (ws, we) = crate::complete::word_at(&chars, cc);
            let base: String = chars[ws..we].iter().collect();
            let ext = app.docs[ai].ext();
            let kw = crate::complete::keywords(&ext);
            let items = crate::complete::filter(kw, &app.word_cache, &base, 24);
            // v1.6: snippet recommendations from the Code Recommendations plugin
            let snips = if app.plugin_enabled("code-recommend") {
                crate::recco::filter_snips(&ext, &base, 6)
            } else {
                Vec::new()
            };
            let hide = items.is_empty()
                && snips.is_empty()
                || (items.len() == 1 && items[0].eq_ignore_ascii_case(&base));
            if hide || base.chars().count() < 2 {
                app.suggest = None;
            } else if let Some(pos) = anchor_pos {
                let same_doc = app
                    .suggest
                    .as_ref()
                    .map(|s| s.doc == ai)
                    .unwrap_or(false);
                if same_doc {
                    if let Some(sg) = app.suggest.as_mut() {
                        sg.items = items;
                        sg.snips = snips;
                        sg.base = base;
                        sg.pos = pos;
                        if sg.sel >= sg.total() {
                            sg.sel = 0;
                        }
                    }
                } else if editor_edited && !suppressed {
                    app.suggest = Some(crate::complete::Suggest {
                        items,
                        sel: 0,
                        base,
                        pos,
                        doc: ai,
                        snips,
                    });
                }
            } else {
                app.suggest = None;
            }
        }
    } else {
        app.suggest = None;
    }

    // ---- gutter numbers + current line highlight (painted on top of fixed strip) ----
    let p = ui.painter_at(clip);
    let total_lines = 1 + app.docs[ai].text.matches('\n').count();
    // the cursor's ROW (for highlight y) and its logical LINE (for numbers):
    // with wrap they differ (#11)
    let cursor_line_row = cursor_rc.map(|(r, _)| r);
    let cursor_line_no = cursor_char_idx
        .map(|cc| crate::util::char_to_line_col(&app.docs[ai].text, cc).0);

    // v1.5 (#11): row -> line mapping from the galley. With word wrap ON one
    // logical line spans MULTIPLE rows; the old `i * row_h` math drew line
    // numbers/current-line/breakpoints at the wrong heights. The galley knows
    // the true row layout (each row ends_with_newline when it closes a line).
    let mut row_line: Vec<usize> = Vec::new();
    if let Some(g) = &galley_ref {
        let mut line = 0usize;
        for row in g.rows.iter() {
            row_line.push(line);
            if row.ends_with_newline {
                line += 1;
            }
        }
    }
    if row_line.is_empty() {
        // no galley this frame — fall back to the identity map (no wrap)
        row_line = (0..total_lines).collect();
    }
    let total_rows = row_line.len();

    // current line soft highlight (semi transparent, drawn over editor bg)
    if let Some(crow) = cursor_line_row {
        let y = galley_top + crow as f32 * row_h;
        if y >= clip.top() && y <= clip.bottom() {
            p.rect_filled(
                egui::Rect::from_min_size(pos2(clip.left(), y), vec2(clip.width(), row_h)),
                0.0,
                Color32::from_rgba_unmultiplied(t.text.r(), t.text.g(), t.text.b(), 12),
            );
        }
    }

    // gutter separator
    p.line_segment(
        [
            pos2(clip.left() + gutter_w, clip.top()),
            pos2(clip.left() + gutter_w, clip.bottom()),
        ],
        Stroke::new(1.0, t.border),
    );

    let first = (((clip.top() - galley_top) / row_h).floor().max(0.0)) as usize;
    let last = (((clip.bottom() - galley_top) / row_h).ceil() as usize + 1).min(total_rows);
    for r in first..last.saturating_add(1) {
        if r >= total_rows {
            break;
        }
        let line_no = row_line[r];
        if line_no >= total_lines {
            break;
        }
        // draw the number only on the FIRST row of the line (wrapped
        // continuation rows stay unnumbered, like VS Code)
        let is_line_start = r == 0 || row_line[r - 1] != line_no;
        let y = galley_top + r as f32 * row_h;
        if y + row_h < clip.top() || y > clip.bottom() {
            continue;
        }
        if is_line_start {
            let col = if cursor_line_no == Some(line_no) { t.accent } else { t.text_faint };
            p.text(
                pos2(clip.left() + gutter_w - 12.0, y + row_h * 0.5),
                Align2::CENTER_CENTER,
                &(line_no + 1).to_string(),
                FontId::monospace(12.0),
                col,
            );
            // diagnostic severity mark in the gutter
            if let Some(dpath) = &app.docs[ai].path {
                if let Some(list) = app.diags.get(dpath) {
                    let has_err = list.iter().any(|d| {
                        let (dl, _) = crate::util::char_to_line_col(&app.docs[ai].text, d.range.0);
                        dl == line_no && d.severity <= 2
                    });
                    if has_err {
                        let c = if list.iter().any(|d| {
                            let (dl, _) =
                                crate::util::char_to_line_col(&app.docs[ai].text, d.range.0);
                            dl == line_no && d.severity == 1
                        }) {
                            t.err
                        } else {
                            t.warn
                        };
                        p.circle_filled(pos2(clip.left() + 20.0, y + row_h * 0.5), 3.0, c);
                    }
                }
            }
            // v1.9: bookmark flag (Ctrl+F2) — an accent tab in the margin
            if let Some(bpath) = &app.docs[ai].path {
                if app
                    .bookmarks
                    .get(bpath)
                    .map(|v| v.binary_search(&line_no).is_ok())
                    .unwrap_or(false)
                {
                    p.rect_filled(
                        egui::Rect::from_min_size(
                            pos2(clip.left() + 30.0, y + row_h * 0.5 - 5.0),
                            vec2(10.0, 10.0),
                        ),
                        2.0,
                        t.accent,
                    );
                }
            }
        }
        // breakpoint dot (debugger) — every row of the line shows it
        if let Some(dpath) = &app.docs[ai].path {
            if app
                .breakpoints
                .get(dpath)
                .map(|b| b.iter().any(|bp| bp.line == line_no))
                .unwrap_or(false)
            {
                p.circle_filled(
                    pos2(clip.left() + 10.0, y + row_h * 0.5),
                    4.0,
                    t.err,
                );
            }
        }
    }

    // v1.9: click the gutter margin to toggle a bookmark on that line
    // (the marker column: breakpoints at +10, diagnostics at +20, bookmarks
    // at +30 — clicking anywhere in the margin toggles)
    let gresp = ui.interact(
        egui::Rect::from_min_size(clip.min, vec2(gutter_w, clip.height())),
        egui::Id::new(format!("rea_gutter_bm_{ai}{suffix}")),
        Sense::click(),
    );
    if gresp.clicked() {
        if let Some(pos) = gresp.interact_pointer_pos() {
            let row = ((pos.y - galley_top) / row_h).floor().max(0.0) as usize;
            if let Some(&line_no) = row_line.get(row) {
                if line_no < total_lines {
                    app.toggle_bookmark_line(ai, line_no);
                }
            }
        }
    }

    // ---- v1.4: find-match highlights + diagnostics squiggles (over the galley) ----
    if let Some(galley) = &galley_ref {
        let gpos = galley_pos_out;
        // visible char window derived from the ROW range via the row->line map
        let first_line = row_line.get(first).copied().unwrap_or(0);
        let last_line = row_line
            .get(last.saturating_sub(1))
            .copied()
            .unwrap_or(total_lines)
            .saturating_add(1);
        // find matches
        if app.find.open && !app.find.query.is_empty() {
            // only paint matches inside the visible viewport
            let vis_start = crate::util::line_col_to_char(&app.docs[ai].text, first_line.saturating_sub(2), 0);
            let vis_end = crate::util::line_col_to_char(&app.docs[ai].text, (last_line + 2).min(total_lines), 0);
            for (mi, (s, e)) in app.find.matches.iter().enumerate() {
                if *e < vis_start || *s > vis_end {
                    continue;
                }
                let active = mi == app.find.idx;
                let col = if active {
                    Color32::from_rgba_unmultiplied(t.accent.r(), t.accent.g(), t.accent.b(), 110)
                } else {
                    Color32::from_rgba_unmultiplied(t.accent.r(), t.accent.g(), t.accent.b(), 45)
                };
                paint_char_range(&p, galley, gpos, *s, *e, col, false);
            }
        }
        // multi-region highlights + secondary CARETS (#6)
        if let Some(m) = &app.multi {
            if m.doc == ai {
                for (s, e) in &m.regions {
                    if s == e {
                        // zero-width region = a secondary cursor: paint a caret bar
                        let cur = galley.from_ccursor(egui::text::CCursor::new(*s));
                        let r = galley.pos_from_cursor(&cur);
                        let x = gpos.x + r.left();
                        let y0 = gpos.y + r.top();
                        let y1 = gpos.y + r.bottom();
                        p.line_segment(
                            [pos2(x, y0), pos2(x, y1)],
                            Stroke::new(1.6, t.text),
                        );
                    } else {
                        paint_char_range(
                            &p,
                            galley,
                            gpos,
                            *s,
                            *e,
                            Color32::from_rgba_unmultiplied(t.warn.r(), t.warn.g(), t.warn.b(), 70),
                            false,
                        );
                    }
                }
            }
        }
        // v2.0: column-selection block highlight (per line, galley-accurate)
        if let Some(cs) = &app.colsel {
            if cs.doc == ai {
                let text = app.docs[ai].text.clone();
                let (la, lb) = cs.lines();
                let (c0, c1) = cs.cols();
                for line in la..=lb {
                    let s0 = crate::util::line_col_to_char(&text, line, c0);
                    let e0 = crate::util::line_col_to_char(&text, line, c1);
                    if e0 > s0 {
                        paint_char_range(
                            &p,
                            galley,
                            gpos,
                            s0,
                            e0,
                            Color32::from_rgba_unmultiplied(
                                t.accent.r(),
                                t.accent.g(),
                                t.accent.b(),
                                80,
                            ),
                            false,
                        );
                    } else {
                        // zero-width column = a caret bar per line
                        let cur = galley.from_ccursor(egui::text::CCursor::new(s0));
                        let r = galley.pos_from_cursor(&cur);
                        let x = gpos.x + r.left();
                        let y0 = gpos.y + r.top();
                        let y1 = gpos.y + r.bottom();
                        p.line_segment(
                            [pos2(x, y0), pos2(x, y1)],
                            Stroke::new(1.6, t.accent),
                        );
                    }
                }
            }
        }
        // diagnostics squiggle underlines
        if let Some(dpath) = &app.docs[ai].path {
            if let Some(list) = app.diags.get(dpath).cloned() {
                for d in list.iter().take(400) {
                    if d.range.1 <= d.range.0 {
                        continue;
                    }
                    let col = if d.severity <= 1 { t.err } else if d.severity == 2 { t.warn } else { t.text_faint };
                    paint_char_range(&p, galley, gpos, d.range.0, d.range.1, col, true);
                }
            }
        }
    }

    // ---- gutter click zones: toggle breakpoints (drawn over the fixed strip).
    // Right-click offers the conditional-breakpoint editor (#16).
    if let Some(dpath) = app.docs[ai].path.clone() {
        for r in first..last.min(total_rows) {
            let line_no = row_line[r];
            if line_no >= total_lines {
                break;
            }
            let y = galley_top + r as f32 * row_h;
            if y + row_h < clip.top() || y > clip.bottom() {
                continue;
            }
            let rect = egui::Rect::from_min_size(
                pos2(clip.left(), y),
                vec2(gutter_w, row_h),
            );
            let resp = ui
                .allocate_rect(rect, Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if resp.clicked() {
                app.toggle_breakpoint(dpath.clone(), line_no);
            }
            if resp.secondary_clicked() {
                app.edit_breakpoint_condition(dpath.clone(), line_no);
            }
            if resp.hovered() {
                let marks = app
                    .breakpoints
                    .get(&dpath)
                    .and_then(|b| b.iter().find(|bp| bp.line == line_no))
                    .map(|bp| {
                        if bp.condition.is_some() {
                            "Conditional breakpoint — right-click to edit"
                        } else {
                            "Breakpoint — right-click to add a condition"
                        }
                    })
                    .unwrap_or("Click to toggle breakpoint");
                resp.show_tooltip_text(marks);
            }
        }
    }

    // ---- hover-eval (#16): evaluate the word under the mouse while paused
    {
        let paused = app.debug.as_ref().map(|s| {
            s.state == crate::dap::DbgState::Paused
                && s.frames
                    .get(s.current_frame)
                    .map(|f| f.file.as_deref() == Some(app.docs[ai].path.as_deref().unwrap_or(std::path::Path::new(""))))
                    .unwrap_or(true)
        }).unwrap_or(false);
        if paused && editor_focus {
            if let (Some(galley), Some(mouse)) = (&galley_ref, ui.input(|i| i.pointer.hover_pos())) {
                if mouse.x > clip.left() + gutter_w
                    && mouse.x < clip.right()
                    && mouse.y > galley_pos_out.y
                    && mouse.y < galley_pos_out.y + galley.size().y
                {
                    let n = char_from_click(galley, galley_pos_out, mouse);
                    let chars: Vec<char> = app.docs[ai].text.chars().collect();
                    let (ws, we) = crate::complete::word_at(&chars, n.min(chars.len()));
                    if we > ws {
                        let word: String = chars[ws..we].iter().collect();
                        let wpos = pos2(mouse.x, mouse.y);
                        app.debug_hover_eval(word.clone(), wpos);
                        // show the latest result as a tooltip near the mouse
                        if let Some((expr, val)) = &app.debug.as_ref().and_then(|s| s.hover_result.clone()) {
                            if *expr == word {
                                egui::show_tooltip(
                                    ui.ctx(),
                                    ui.layer_id(),
                                    egui::Id::new("rea_hover_eval"),
                                    |ui| {
                                        ui.monospace(
                                            egui::RichText::new(format!("{} = {}", expr, val)).size(11.0),
                                        );
                                    },
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    // ---- v1.7: minimap (right-edge overview + click/drag to scroll) ----
    if app.settings.minimap && suffix.is_empty() && total_lines > 8 {
        let mw = 92.0f32;
        let mrect = egui::Rect::from_min_size(
            pos2(clip.right() - mw, clip.top()),
            vec2(mw, clip.height()),
        );
        let resp = ui.allocate_rect(mrect, Sense::click_and_drag());
        let mp = ui.painter_at(mrect);
        mp.rect_filled(mrect, 0.0, t.editor_bg);
        mp.line_segment(
            [pos2(mrect.left(), mrect.top()), pos2(mrect.left(), mrect.bottom())],
            Stroke::new(1.0, t.border),
        );

        // line height in the minimap: fit the whole file when possible
        let lh = (clip.height() / total_lines as f32).clamp(1.4, 3.2);
        let fits = total_lines as f32 * lh <= clip.height();
        let first_line_vis = row_line.get(first).copied().unwrap_or(0);
        let draw_from = if fits {
            0usize
        } else {
            // keep a sliding window centered on the viewport
            let win = ((clip.height() / lh) as usize).min(total_lines);
            let center = first_line_vis + (last.saturating_sub(first)) / 2;
            center.saturating_sub(win / 2).min(total_lines.saturating_sub(win))
        };
        let draw_to = (draw_from + ((clip.height() / lh) as usize) + 2).min(total_lines);

        let lines: Vec<&str> = app.docs[ai].text.split('\n').collect();
        for li in draw_from..draw_to {
            let Some(ltext) = lines.get(li) else { break };
            let trimmed = ltext.trim_start();
            if trimmed.is_empty() {
                continue;
            }
            let col = if trimmed.starts_with("//") || trimmed.starts_with('#')
                || trimmed.starts_with("--") || trimmed.starts_with("/*")
            {
                t.ok
            } else if trimmed.starts_with("fn ")
                || trimmed.starts_with("def ")
                || trimmed.starts_with("class ")
                || trimmed.starts_with("function ")
                || trimmed.starts_with("func ")
                || trimmed.starts_with("public ")
                || trimmed.starts_with("private ")
                || trimmed.starts_with("import ")
                || trimmed.starts_with("use ")
                || trimmed.starts_with("if ")
                || trimmed.starts_with("for ")
                || trimmed.starts_with("while ")
            {
                t.accent
            } else {
                t.text_dim
            };
            let wpx = (trimmed.chars().count() as f32 * 1.15).min(mw - 10.0);
            let y = mrect.top() + (li - draw_from) as f32 * lh;
            mp.rect_filled(
                egui::Rect::from_min_size(
                    pos2(mrect.left() + 4.0, y),
                    vec2(wpx, (lh - 0.7).max(0.8)),
                ),
                0.0,
                col.gamma_multiply(0.5),
            );
        }

        // viewport overlay
        let vp_first = row_line.get(first).copied().unwrap_or(0);
        let vp_last = row_line.get(last.saturating_sub(1)).copied().unwrap_or(total_lines);
        let vy = mrect.top() + (vp_first.saturating_sub(draw_from)) as f32 * lh;
        let vh = (vp_last.saturating_sub(vp_first) + 1) as f32 * lh;
        let vrect = egui::Rect::from_min_size(pos2(mrect.left() + 1.0, vy), vec2(mw - 2.0, vh));
        mp.rect_filled(vrect, 2.0, Color32::from_rgba_unmultiplied(t.accent.r(), t.accent.g(), t.accent.b(), 22));
        mp.rect_stroke(vrect, 2.0, Stroke::new(1.0, t.accent.gamma_multiply(0.7)));

        // click / drag → scroll the editor
        if resp.clicked() || resp.dragged() {
            if let Some(pos) = resp.interact_pointer_pos() {
                let target = ((pos.y - mrect.top()) / lh) as isize + draw_from as isize;
                let target = target.max(0) as usize;
                let sa_id = egui::Id::new(format!("editor_{ai}{suffix}"));
                if let Some(mut st) = egui::scroll_area::State::load(ui.ctx(), sa_id) {
                    st.offset.y = target as f32 * row_h - clip.height() * 0.35;
                    st.store(ui.ctx(), sa_id);
                }
            }
        }
    }

    // store cursor for status bar
    app.cursor = cursor_rc; // (row, col) — row→line resolved by callers when needed
    app.cursor_line_no = cursor_line_no;
}

// ---------------------------------------------------------------- hit testing

/// Map a click position to a char index through the galley (used by Alt+Click
/// multi-cursor and hover-eval). Pure public-API implementation: walk the
/// rows by y, then the glyphs by x.
fn char_from_click(galley: &egui::Galley, gpos: egui::Pos2, click: egui::Pos2) -> usize {
    let rel_y = click.y - gpos.y;
    let rel_x = click.x - gpos.x;
    let n_rows = galley.rows.len();
    let mut idx = 0usize;
    for (ri, row) in galley.rows.iter().enumerate() {
        let row_chars = row.glyphs.len() + if row.ends_with_newline { 1 } else { 0 };
        let is_last = ri + 1 == n_rows;
        // is the click on a row BELOW this one?
        if !is_last && rel_y > row.rect.bottom() {
            idx += row_chars;
            continue;
        }
        // clicked on this row (or the last one)
        for g in &row.glyphs {
            if rel_x < g.pos.x + g.advance_width * 0.5 {
                return idx;
            }
            idx += 1;
        }
        if row.ends_with_newline {
            idx += 1; // position on/after the newline
        }
        return idx;
    }
    idx
}

/// Paint a highlight (or squiggle) over a char range of the galley,
/// handling ranges that span multiple rows.
fn paint_char_range(
    p: &egui::Painter,
    galley: &egui::Galley,
    gpos: egui::Pos2,
    s: usize,
    e: usize,
    color: Color32,
    squiggle: bool,
) {
    let total = galley.text().chars().count();
    if galley.is_empty() {
        return;
    }
    let s = s.min(total);
    let e = e.min(total).max(s + 1);
    let cur_s = galley.from_ccursor(egui::text::CCursor::new(s));
    let cur_e = galley.from_ccursor(egui::text::CCursor::new(e));
    let rs = galley.pos_from_cursor(&cur_s);
    let re = galley.pos_from_cursor(&cur_e);

    if (rs.top() - re.top()).abs() < 0.5 {
        // same row
        let rect = egui::Rect::from_min_max(
            gpos + vec2(rs.left(), rs.top()),
            gpos + vec2(re.left().max(rs.left() + 2.0), re.bottom()),
        );
        if squiggle {
            draw_squiggle(p, rect, color);
        } else {
            p.rect_filled(rect, 2.0, color);
        }
        return;
    }
    // multi-row: fill from start to end of its row, full rows between,
    // then from row start to end position
    let row_start_bottom = rs.bottom();
    let first = egui::Rect::from_min_max(
        gpos + vec2(rs.left(), rs.top()),
        gpos + vec2(rs.right().max(rs.left() + 2.0), row_start_bottom),
    );
    if squiggle {
        draw_squiggle(p, first, color);
    } else {
        p.rect_filled(first, 2.0, color);
    }
    // middle rows: use the galley's row rects
    let rows = galley.rows.len();
    for (ri, row) in galley.rows.iter().enumerate() {
        let row_top = gpos.y + row.rect.top();
        if row_top < rs.bottom() - 0.5 || row_top > re.top() + 0.5 {
            continue;
        }
        let mid = egui::Rect::from_min_max(
            gpos + vec2(row.rect.left(), row.rect.top()),
            gpos + vec2(row.rect.right(), row.rect.bottom()),
        );
        if squiggle {
            draw_squiggle(p, mid, color);
        } else {
            p.rect_filled(mid, 2.0, color);
        }
        let _ = ri;
    }
    let last = egui::Rect::from_min_max(
        gpos + vec2(re.left(), re.top()),
        gpos + vec2(re.left() + 2.0, re.bottom()),
    );
    let _ = last;
    if squiggle {
        draw_squiggle(p, last, color);
    } else {
        p.rect_filled(last, 2.0, color);
    }
}

fn draw_squiggle(p: &egui::Painter, rect: egui::Rect, col: Color32) {
    let y = rect.bottom() - 1.5;
    let mut x = rect.left();
    let mut up = true;
    while x < rect.right() {
        let nx = (x + 3.0).min(rect.right());
        let ny = if up { y - 2.0 } else { y };
        p.line_segment([pos2(x, y), pos2(nx, ny)], Stroke::new(1.0, col));
        x = nx;
        up = !up;
    }
}

// ---------------------------------------------------------------- floating find box

/// Compact, centered, floating find box — sits on top of the file content,
/// like VS Code's find widget, with rounded corners + soft shadow.
/// v1.4: replace row (Ctrl+H), case / whole-word / regex options,
/// and match highlights painted in the text.
fn find_overlay(app: &mut App, ctx: &egui::Context, t: &crate::theme::Theme) {
    let er = app.editor_rect;
    if er.width() < 60.0 {
        return;
    }
    let w = 560.0_f32.min(er.width() - 40.0).max(300.0);
    // center over the EDITOR area (excludes sidebar), not the whole window
    let pos = pos2(er.center().x - w * 0.5, er.top() + 10.0);

    egui::Area::new(egui::Id::new("rea_find_box"))
        .order(egui::Order::Foreground)
        .fixed_pos(pos)
        .movable(false)
        .show(ctx, |ui| {
            egui::Frame::default()
                .fill(t.find_bg)
                .stroke(Stroke::new(1.0, t.border))
                .rounding(Rounding::same(10.0))
                .inner_margin(egui::Margin::symmetric(10.0, 7.0))
                .shadow(egui::Shadow {
                    offset: vec2(0.0, 6.0),
                    blur: 20.0,
                    spread: 0.0,
                    color: t.shadow_col,
                })
                .show(ui, |ui| {
                    ui.set_max_width(w - 20.0);
                    ui.set_min_width(w - 20.0);

                    // recompute matches if needed
                    app.find_maybe_recompute();

                    let total = app.find.matches.len();
                    let cur = if total == 0 { 0 } else { app.find.idx + 1 };

                    // ---- row 1: query + options + nav ----
                    ui.horizontal(|ui| {
                        let (ipos, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                        icons::magnifier(&ui.painter_at(ipos), ipos.center(), t.text_dim);

                        let mut q = app.find.query.clone();
                        let resp = egui::TextEdit::singleline(&mut q)
                            .desired_width(200.0)
                            .frame(false)
                            .hint_text("Find")
                            .font(TextStyle::Body)
                            .show(ui);
                        if resp.response.changed() {
                            app.find.query = q;
                            app.find.sig = None;
                        }
                        app.find.input_focused = resp.response.has_focus();
                        if app.find.focus_req {
                            resp.response.request_focus();
                            app.find.focus_req = false;
                        }

                        // option toggles (toggle_value already flips the flag —
                        // the old code flipped it a second time, so the toggle
                        // never stuck)
                        if ui.toggle_value(&mut app.find.case_sensitive, "Aa").on_hover_text("Match case").changed() {
                            app.find.sig = None;
                        }
                        if ui.toggle_value(&mut app.find.whole_word, "ab").on_hover_text("Whole word").changed() {
                            app.find.sig = None;
                        }
                        if ui.toggle_value(&mut app.find.use_regex, ".*").on_hover_text("Regular expression").changed() {
                            app.find.sig = None;
                        }

                        // count
                        let msg = if app.find.error.is_some() {
                            egui::RichText::new("Bad pattern").size(11.0).color(t.err)
                        } else if app.find.query.is_empty() {
                            egui::RichText::new("").size(12.0)
                        } else if total == 0 {
                            egui::RichText::new("No results").size(12.0).color(t.err)
                        } else {
                            egui::RichText::new(format!("{cur} of {total}"))
                                .size(12.0)
                                .color(t.text_dim)
                        };
                        ui.label(msg);

                        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                            // close
                            let (cp, cr) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
                            if cr.hovered() {
                                ui.painter().rect_filled(cp, 4.0, t.hover);
                            }
                            icons::close_x(&ui.painter(), cp.center(), t.text_dim);
                            if cr.clicked() {
                                app.close_find();
                            }
                            // replace toggle
                            let (rp, rr) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
                            if rr.hovered() {
                                ui.painter().rect_filled(rp, 4.0, t.hover);
                            }
                            let pc = ui.painter();
                            let c = rp.center();
                            pc.line_segment([c + vec2(-6.0, -3.0), c + vec2(6.0, -3.0)], Stroke::new(1.3, t.text_dim));
                            pc.line_segment([c + vec2(-6.0, 3.0), c + vec2(6.0, 3.0)], Stroke::new(1.3, t.text_dim));
                            pc.line_segment([c + vec2(2.0, 1.0), c + vec2(6.0, 3.0)], Stroke::new(1.3, t.text_dim));
                            pc.line_segment([c + vec2(2.0, 5.0), c + vec2(6.0, 3.0)], Stroke::new(1.3, t.text_dim));
                            if rr.clicked() {
                                app.find.show_replace = !app.find.show_replace;
                                if app.find.show_replace {
                                    app.find.replace_focus_req = true;
                                }
                            }
                            // prev
                            let (pp, pr) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
                            if pr.hovered() {
                                ui.painter().rect_filled(pp, 4.0, t.hover);
                            }
                            icons::arrow(&ui.painter(), pp.center(), false, t.text_dim);
                            if pr.clicked() {
                                app.find_prev();
                            }
                            // next
                            let (np, nr) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
                            if nr.hovered() {
                                ui.painter().rect_filled(np, 4.0, t.hover);
                            }
                            icons::arrow(&ui.painter(), np.center(), true, t.text_dim);
                            if nr.clicked() {
                                app.find_next();
                            }
                        });
                    });

                    // ---- row 2: replace (revealed) ----
                    if app.find.show_replace {
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            let (ipos, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                            let pc = ui.painter_at(ipos);
                            let c = ipos.center();
                            pc.line_segment([c + vec2(-6.0, -3.0), c + vec2(6.0, -3.0)], Stroke::new(1.3, t.text_dim));
                            pc.line_segment([c + vec2(-6.0, 3.0), c + vec2(6.0, 3.0)], Stroke::new(1.3, t.text_dim));
                            pc.line_segment([c + vec2(2.0, 1.0), c + vec2(6.0, 3.0)], Stroke::new(1.3, t.text_dim));
                            pc.line_segment([c + vec2(2.0, 5.0), c + vec2(6.0, 3.0)], Stroke::new(1.3, t.text_dim));

                            let mut rp = app.find.replace.clone();
                            let resp = egui::TextEdit::singleline(&mut rp)
                                .desired_width(200.0)
                                .frame(false)
                                .hint_text("Replace with")
                                .font(TextStyle::Body)
                                .show(ui);
                            if resp.response.changed() {
                                app.find.replace = rp;
                            }
                            if app.find.replace_focus_req {
                                resp.response.request_focus();
                                app.find.replace_focus_req = false;
                            }
                            if ui.add_enabled(total > 0, egui::Button::new(egui::RichText::new("Replace").size(11.5))).clicked() {
                                app.find_replace_one();
                            }
                            if ui.add_enabled(total > 0, egui::Button::new(egui::RichText::new("All").size(11.5))).clicked() {
                                app.find_replace_all();
                            }
                        });
                    }
                });
        });
}

// ---------------------------------------------------------------- suggestion popup

/// VS Code-like autocomplete popup, anchored right below the caret.
/// Up/Down select, Enter/Tab accept, Esc closes (keys handled in App).
fn suggest_popup(app: &mut App, ctx: &egui::Context, t: &crate::theme::Theme) {
    let Some(sg) = app.suggest.clone() else { return };
    let screen = ctx.screen_rect();
    let w = 360.0_f32.min(screen.width() - 40.0);
    let row_h = 21.0;
    let max_items = 9;
    let list_h = (sg.total().min(max_items) as f32) * row_h + 8.0;

    let mut pos = sg.pos + vec2(6.0, 4.0);
    if pos.y + list_h + 26.0 > screen.bottom() {
        pos.y = (sg.pos.y - list_h - 24.0).max(screen.top() + 4.0);
    }
    pos.x = pos.x.clamp(screen.left() + 4.0, (screen.right() - w - 12.0).max(screen.left() + 4.0));

    let mut accept_idx: Option<usize> = None;
    let mut hover_idx: Option<usize> = None;

    egui::Area::new(egui::Id::new("rea_suggest"))
        .order(egui::Order::Foreground)
        .fixed_pos(pos)
        .movable(false)
        .show(ctx, |ui| {
            egui::Frame::default()
                .fill(t.bg)
                .stroke(Stroke::new(1.0, t.border))
                .rounding(Rounding::same(8.0))
                .inner_margin(egui::Margin::same(4.0))
                .shadow(egui::Shadow {
                    offset: vec2(0.0, 4.0),
                    blur: 14.0,
                    spread: 0.0,
                    color: t.shadow_col,
                })
                .show(ui, |ui| {
                    ui.set_max_width(w);
                    ui.set_min_width(w - 8.0);
                    egui::ScrollArea::vertical()
                        .max_height(list_h)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            // total rows = snippet recommendations (◆) + words
                            let total = sg.total();
                            for vi in 0..total {
                                let selected = vi == sg.sel;
                                // snippet row?
                                let is_snip = vi < sg.snips.len();
                                let fr = egui::Frame::default()
                                    .fill(if selected {
                                        t.active_row
                                    } else {
                                        Color32::TRANSPARENT
                                    })
                                    .rounding(Rounding::same(5.0))
                                    .inner_margin(egui::Margin::symmetric(8.0, 2.0))
                                    .show(ui, |ui| {
                                        ui.set_min_width(w - 30.0);
                                        ui.horizontal(|ui| {
                                            if is_snip {
                                                let s = &sg.snips[vi];
                                                ui.label(
                                                    egui::RichText::new("◆")
                                                        .size(11.0)
                                                        .color(if selected { t.accent } else { t.accent.gamma_multiply(0.7) }),
                                                );
                                                ui.label(
                                                    egui::RichText::new(s.label)
                                                        .font(FontId::monospace(12.0))
                                                        .strong()
                                                        .color(if selected { t.accent } else { t.text }),
                                                );
                                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                    ui.label(
                                                        egui::RichText::new(s.desc)
                                                            .size(9.5)
                                                            .color(t.text_faint),
                                                    );
                                                });
                                            } else {
                                                let wi = vi - sg.snips.len();
                                                let item = &sg.items[wi];
                                                let prefix: String =
                                                    item.chars().take(sg.base.chars().count()).collect();
                                                let rest: String =
                                                    item.chars().skip(sg.base.chars().count()).collect();
                                                ui.label(
                                                    egui::RichText::new(&prefix)
                                                        .font(FontId::monospace(12.5))
                                                        .strong()
                                                        .color(if selected { t.accent } else { t.text }),
                                                );
                                                ui.label(
                                                    egui::RichText::new(&rest)
                                                        .font(FontId::monospace(12.5))
                                                        .color(if selected { t.text } else { t.text_dim }),
                                                );
                                            }
                                        });
                                    });
                                let resp = fr.response.interact(Sense::click());
                                if resp.hovered() {
                                    hover_idx = Some(vi);
                                }
                                if resp.clicked() {
                                    accept_idx = Some(vi);
                                }
                                resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                            }
                        });
                });
        });

    if let Some(hi) = hover_idx {
        if let Some(s2) = app.suggest.as_mut() {
            s2.sel = hi;
        }
    }
    if let Some(vi) = accept_idx {
        if let Some(s2) = app.suggest.as_mut() {
            s2.sel = vi;
        }
        app.accept_suggestion(ctx);
    }
}
