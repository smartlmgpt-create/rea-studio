use eframe::egui;
use egui::{Align, Align2, Color32, Rounding, Sense, Stroke, vec2};

use crate::app::App;
use crate::theme::Theme;

#[derive(Clone, Copy, PartialEq)]
pub enum PaletteMode {
    Commands,
    Files,
    /// Ctrl+G — jump to a line number (#9)
    GotoLine,
}

#[derive(Clone)]
pub struct PalItem {
    pub id: String,   // command id or "open:<path>"
    pub label: String,
    pub hint: String,
}

/// The command palette / quick-open overlay.
/// Rendered near the top-center, rounded, soft shadow, fully theme aware.
/// Keyboard: type to filter, Up/Down to select, Enter to run, Esc to close.
/// (Keys are consumed at the top of App::update so the text field never fights them.)
pub struct Palette {
    pub mode: PaletteMode,
    pub query: String,
    pub items: Vec<PalItem>,
    pub sel: usize,
    pub focus_req: bool,
    pub filtered: Vec<usize>,
    /// rect from last frame — used for "click outside closes"
    pub last_rect: egui::Rect,
}

impl Palette {
    pub fn new(mode: PaletteMode) -> Self {
        Self {
            mode,
            query: String::new(),
            items: Vec::new(),
            sel: 0,
            focus_req: true,
            filtered: Vec::new(),
            last_rect: egui::Rect::NOTHING,
        }
    }

    pub fn refilter(&mut self) {
        // GotoLine: the query IS the line number — exact numeric filter
        if self.mode == PaletteMode::GotoLine {
            let q = self.query.trim();
            self.filtered.clear();
            if q.is_empty() {
                self.filtered = (0..self.items.len().min(200)).collect();
            } else {
                for (i, it) in self.items.iter().enumerate() {
                    let n = it.label.trim_start_matches("Ln ");
                    if n.starts_with(q) {
                        self.filtered.push(i);
                    }
                    if self.filtered.len() >= 200 {
                        break;
                    }
                }
            }
            if self.sel >= self.filtered.len() {
                self.sel = self.filtered.len().saturating_sub(1);
            }
            return;
        }
        let q = self.query.trim().to_lowercase();
        self.filtered.clear();
        if q.is_empty() {
            self.filtered = (0..self.items.len()).collect();
        } else {
            let mut scored: Vec<(i32, usize)> = self
                .items
                .iter()
                .enumerate()
                .filter_map(|(i, it)| fuzzy(&it.label, &q).map(|s| (s, i)))
                .collect();
            scored.sort_by(|a, b| b.0.cmp(&a.0));
            self.filtered = scored.into_iter().map(|(_, i)| i).collect();
        }
        if self.sel >= self.filtered.len() {
            self.sel = self.filtered.len().saturating_sub(1);
        }
    }

    pub fn move_sel(&mut self, dir: i32) {
        if self.filtered.is_empty() {
            return;
        }
        let n = self.filtered.len() as i32;
        self.sel = ((self.sel as i32 + dir).rem_euclid(n)) as usize;
    }

    pub fn current_id(&self) -> Option<String> {
        self.filtered
            .get(self.sel)
            .and_then(|i| self.items.get(*i))
            .map(|it| it.id.clone())
    }
}

/// simple subsequence fuzzy score (higher = better; None = no match)
fn fuzzy(hay: &str, needle: &str) -> Option<i32> {
    let h = hay.to_lowercase();
    let h = h.as_bytes();
    let n = needle.as_bytes();
    let mut hi = 0usize;
    let mut score = 0i32;
    let mut last_hit: i32 = -10;
    for (ni, &nc) in n.iter().enumerate() {
        let mut found = false;
        while hi < h.len() {
            if h[hi] == nc {
                score += if hi as i32 - last_hit == 1 { 3 } else { 1 };
                if ni == 0 && hi == 0 {
                    score += 4;
                }
                last_hit = hi as i32;
                hi += 1;
                found = true;
                break;
            }
            hi += 1;
        }
        if !found {
            return None;
        }
    }
    Some(score)
}

// ---------------------------------------------------------------- render

pub fn show(app: &mut App, ctx: &egui::Context, t: &Theme) {
    let Some(palette) = app.palette.as_mut() else {
        return;
    };
    palette.refilter();
    let mut run_id: Option<String> = None;

    egui::Area::new(egui::Id::new("rea_palette"))
        .order(egui::Order::Foreground)
        .anchor(Align2::CENTER_TOP, [0.0, 56.0])
        .movable(false)
        .show(ctx, |ui| {
            egui::Frame::default()
                .fill(t.bg)
                .stroke(Stroke::new(1.0, t.border))
                .rounding(Rounding::same(12.0))
                .inner_margin(egui::Margin::same(8.0))
                .shadow(egui::Shadow {
                    offset: vec2(0.0, 10.0),
                    blur: 30.0,
                    spread: 2.0,
                    color: t.shadow_col,
                })
                .show(ui, |ui| {
                    // HARD size caps — compact palette, never screen-sized
                    ui.set_min_width(560.0);
                    ui.set_max_width(560.0);

                    // ---- query row ----
                    ui.horizontal(|ui| {
                        let mode_txt = match palette.mode {
                            PaletteMode::Commands => "Commands",
                            PaletteMode::Files => "Go to File",
                            PaletteMode::GotoLine => "Go to Line",
                        };
                        egui::Frame::default()
                            .fill(t.active_row)
                            .rounding(Rounding::same(6.0))
                            .inner_margin(egui::Margin::symmetric(8.0, 3.0))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(mode_txt)
                                        .size(11.0)
                                        .strong()
                                        .color(t.accent_text),
                                );
                            });
                        ui.add_space(4.0);

                        let hint = match palette.mode {
                            PaletteMode::Commands => "Type a command…",
                            PaletteMode::Files => "Search files by name…",
                            PaletteMode::GotoLine => "Line number…",
                        };
                        let mut q = palette.query.clone();
                        let resp = egui::TextEdit::singleline(&mut q)
                            .desired_width(430.0)
                            .frame(false)
                            .hint_text(hint)
                            .font(egui::TextStyle::Body)
                            .show(ui);
                        if resp.response.changed() {
                            palette.query = q;
                            palette.refilter();
                        }
                        if palette.focus_req {
                            resp.response.request_focus();
                            palette.focus_req = false;
                        }
                    });

                    ui.add_space(6.0);
                    ui.separator();
                    ui.add_space(4.0);

                    // ---- results ----
                    egui::ScrollArea::vertical()
                        .max_height(340.0)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            if palette.filtered.is_empty() {
                                let empty_msg = match palette.mode {
                                    PaletteMode::Commands => "No matching commands",
                                    PaletteMode::GotoLine => "No matching line",
                                    PaletteMode::Files => {
                                        if palette.items.is_empty() {
                                            "No files yet — open a folder or a recent file first"
                                        } else {
                                            "No matching files"
                                        }
                                    }
                                };
                                ui.vertical_centered(|ui| {
                                    ui.add_space(10.0);
                                    ui.label(
                                        egui::RichText::new(empty_msg)
                                            .size(13.0)
                                            .color(t.text_faint),
                                    );
                                    ui.add_space(10.0);
                                });
                            } else {
                                for (vi, &ii) in palette.filtered.iter().enumerate() {
                                    if vi >= 60 {
                                        break; // show top 60 matches
                                    }
                                    let item = &palette.items[ii];
                                    let selected = vi == palette.sel;

                                    let fr = egui::Frame::default()
                                        .fill(if selected { t.active_row } else { Color32::TRANSPARENT })
                                        .rounding(Rounding::same(8.0))
                                        .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                                        .show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                ui.label(
                                                    egui::RichText::new(&item.label)
                                                        .size(13.5)
                                                        .color(if selected { t.text } else { t.text }),
                                                );
                                                ui.with_layout(
                                                    egui::Layout::right_to_left(Align::Center),
                                                    |ui| {
                                                        if selected {
                                                            egui::Frame::default()
                                                                .fill(t.accent.gamma_multiply(0.25))
                                                                .rounding(Rounding::same(4.0))
                                                                .inner_margin(egui::Margin::symmetric(6.0, 1.0))
                                                                .show(ui, |ui| {
                                                                    ui.label(
                                                                        egui::RichText::new("Enter")
                                                                            .size(10.0)
                                                                            .color(t.accent_text),
                                                                    );
                                                                });
                                                        }
                                                        ui.label(
                                                            egui::RichText::new(&item.hint)
                                                                .size(11.0)
                                                                .color(t.text_faint),
                                                        );
                                                    },
                                                );
                                            });
                                        });
                                    let resp = fr.response.interact(Sense::click());
                                    if resp.hovered() {
                                        palette.sel = vi;
                                    }
                                    if resp.clicked() {
                                        run_id = Some(item.id.clone());
                                    }
                                }
                            }
                        });

                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("Up/Down navigate  ·  Enter run  ·  Esc close")
                                .size(10.5)
                                .color(t.text_faint),
                        );
                    });

                    palette.last_rect = ui.min_rect();
                });
        });

    // click outside -> close
    let pointer_pos = ctx.input(|i| i.pointer.interact_pos());
    let any_click = ctx.input(|i| i.pointer.any_click());
    if let (Some(pp), true) = (pointer_pos, any_click) {
        let inside = app
            .palette
            .as_ref()
            .map(|p| p.last_rect.expand(6.0).contains(pp))
            .unwrap_or(false);
        if !inside {
            app.palette = None;
        }
    }

    if let Some(id) = run_id {
        let id2 = id.clone();
        app.palette = None;
        app.run_command(&id2, ctx);
    }
}
