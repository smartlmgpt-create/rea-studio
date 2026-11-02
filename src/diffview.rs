//! v2.1: side-by-side file compare (the Notepad++ "Compare" counterpart).
//!
//! Myers O(ND) line diff with a bounded D (falls back to plain block pairing
//! on pathological inputs), rendered as two synchronized columns in a single
//! row-aligned ScrollArea — equal / inserted / deleted / modified rows get
//! distinct backgrounds. F8 / Shift+F8 jump between hunks, Esc closes.

use std::path::PathBuf;

use crate::app::App;

const MAX_LINES: usize = 30_000;
const MAX_D: usize = 1500; // Myers bound → ~11 MB worst-case backtrack memory

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowKind {
    Eq,
    Del, // only on the left
    Ins, // only on the right
    Mod, // both sides changed
}

#[derive(Clone, Copy, Debug)]
pub struct Row {
    pub kind: RowKind,
    pub l: Option<usize>,
    pub r: Option<usize>,
}

#[derive(Clone, Copy)]
enum Step {
    Eq(usize, usize),
    Del(usize),
    Ins(usize),
}

/// Classic Myers shortest-edit-script with backtracking. Returns `None` when
/// the edit distance exceeds `MAX_D` (caller falls back to block pairing).
fn myers_steps(a: &[String], b: &[String]) -> Option<Vec<Step>> {
    let n = a.len();
    let m = b.len();
    let max = n + m;
    if max == 0 {
        return Some(Vec::new());
    }
    let off = max as isize;
    let mut trace: Vec<Vec<isize>> = Vec::new();
    let mut v = vec![0isize; 2 * max + 1];
    let mut found: Option<usize> = None;
    for d in 0..=MAX_D {
        trace.push(v.clone());
        let di = d as isize;
        let mut k = -di;
        while k <= di {
            let ki = (k + off) as usize;
            let mut x = if k == -di || (k != di && v[ki - 1] < v[ki + 1]) {
                v[ki + 1]
            } else {
                v[ki - 1] + 1
            };
            let mut y = x - k;
            while (x as usize) < n && (y as usize) < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[ki] = x;
            if x as usize >= n && y as usize >= m {
                found = Some(d);
                break;
            }
            k += 2;
        }
        if found.is_some() {
            break;
        }
    }
    let d_final = found?;

    let mut steps_rev: Vec<Step> = Vec::new();
    let mut x = n as isize;
    let mut y = m as isize;
    for d in (0..=d_final).rev() {
        let vd = &trace[d];
        let di = d as isize;
        let k = x - y;
        let ki = (k + off) as usize;
        let prev_k = if k == -di || (k != di && vd[ki - 1] < vd[ki + 1]) {
            k + 1
        } else {
            k - 1
        };
        let prev_x = vd[(prev_k + off) as usize];
        let prev_y = prev_x - prev_k;
        // diagonal run of step d (walked backwards)
        while x > prev_x && y > prev_y {
            x -= 1;
            y -= 1;
            steps_rev.push(Step::Eq(x as usize, y as usize));
        }
        if d > 0 {
            if x == prev_x {
                y -= 1;
                steps_rev.push(Step::Ins(y as usize));
            } else {
                x -= 1;
                steps_rev.push(Step::Del(x as usize));
            }
        }
        x = prev_x;
        y = prev_y;
    }
    steps_rev.reverse();
    Some(steps_rev)
}

/// Fallback when Myers is out of budget: pair what is pairable, mark the rest.
fn fallback_steps(a: &[String], b: &[String]) -> Vec<Step> {
    let mut out = Vec::new();
    let common = a.len().min(b.len());
    for i in 0..common {
        if a[i] == b[i] {
            out.push(Step::Eq(i, i));
        } else {
            out.push(Step::Del(i));
            out.push(Step::Ins(i));
        }
    }
    for i in common..a.len() {
        out.push(Step::Del(i));
    }
    for i in common..b.len() {
        out.push(Step::Ins(i));
    }
    out
}

/// Merge adjacent Del/Ins runs into `Mod` rows (k-th deleted line pairs with
/// the k-th inserted line) and produce the aligned row list.
pub fn rows_from_steps(steps: &[Step]) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    let mut i = 0;
    while i < steps.len() {
        match steps[i] {
            Step::Eq(l, r) => {
                rows.push(Row { kind: RowKind::Eq, l: Some(l), r: Some(r) });
                i += 1;
            }
            Step::Del(_) | Step::Ins(_) => {
                // collect the run
                let mut dels: Vec<usize> = Vec::new();
                let mut inss: Vec<usize> = Vec::new();
                while i < steps.len() {
                    match steps[i] {
                        Step::Del(l) => dels.push(l),
                        Step::Ins(r) => inss.push(r),
                        _ => break,
                    }
                    i += 1;
                }
                let pairs = dels.len().min(inss.len());
                for p in 0..pairs {
                    rows.push(Row { kind: RowKind::Mod, l: Some(dels[p]), r: Some(inss[p]) });
                }
                for &l in &dels[pairs..] {
                    rows.push(Row { kind: RowKind::Del, l: Some(l), r: None });
                }
                for &r in &inss[pairs..] {
                    rows.push(Row { kind: RowKind::Ins, l: None, r: Some(r) });
                }
            }
        }
    }
    rows
}

/// Full diff of two line vectors (prefix/suffix trim → Myers → merge).
pub fn compute_diff(a: &[String], b: &[String]) -> Vec<Row> {
    // trim common prefix / suffix — keeps the Myers matrix small
    let mut p = 0;
    while p < a.len() && p < b.len() && a[p] == b[p] {
        p += 1;
    }
    let mut s = 0;
    while s < a.len() - p && s < b.len() - p && a[a.len() - 1 - s] == b[b.len() - 1 - s] {
        s += 1;
    }
    let prefix = (0..p).map(|i| Row { kind: RowKind::Eq, l: Some(i), r: Some(i) });
    let suffix = (0..s).map(move |i| {
        Row { kind: RowKind::Eq, l: Some(a.len() - s + i), r: Some(b.len() - s + i) }
    });
    let am = &a[p..a.len() - s];
    let bm = &b[p..b.len() - s];
    let steps = myers_steps(am, bm).unwrap_or_else(|| fallback_steps(am, bm));
    // middle rows are indexed relative to the trimmed slices → re-base by p
    let mid = rows_from_steps(&steps)
        .into_iter()
        .map(|mut r| {
            if let Some(l) = r.l {
                r.l = Some(l + p);
            }
            if let Some(rr) = r.r {
                r.r = Some(rr + p);
            }
            r
        });
    prefix.chain(mid).chain(suffix).collect()
}

// ---------------------------------------------------------------- view state

pub struct DiffView {
    pub left_name: String,
    pub right_name: String,
    pub left_path: PathBuf,
    pub right_path: PathBuf,
    pub left: Vec<String>,
    pub right: Vec<String>,
    pub rows: Vec<Row>,
    /// row index where each hunk starts (sorted)
    pub hunks: Vec<usize>,
    pub hunk_cur: usize,
    /// pending scroll target row (consumed by the UI pass)
    pub scroll_to: Option<usize>,
    pub ins_count: usize,
    pub del_count: usize,
    pub used_fallback: bool,
}

impl DiffView {
    fn build(
        left_path: PathBuf,
        left: Vec<String>,
        right_path: PathBuf,
        right: Vec<String>,
        used_fallback: bool,
    ) -> Self {
        let rows = compute_diff(&left, &right);
        let hunks: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter(|(i, r)| r.kind != RowKind::Eq && (*i == 0 || rows[i - 1].kind == RowKind::Eq))
            .map(|(i, _)| i)
            .collect();
        let ins_count = rows.iter().filter(|r| matches!(r.kind, RowKind::Ins | RowKind::Mod)).count();
        let del_count = rows.iter().filter(|r| matches!(r.kind, RowKind::Del | RowKind::Mod)).count();
        let name = |p: &PathBuf| {
            p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| p.display().to_string())
        };
        let first_hunk = hunks.first().copied();
        Self {
            left_name: name(&left_path),
            right_name: name(&right_path),
            left_path,
            right_path,
            left,
            right,
            rows,
            hunks,
            hunk_cur: 0,
            scroll_to: first_hunk,
            ins_count,
            del_count,
            used_fallback,
        }
    }

    /// Row span (start..end) of the current hunk, for the margin markers.
    fn hunk_range(&self, idx: usize) -> std::ops::Range<usize> {
        if self.hunks.is_empty() {
            return 0..0;
        }
        let i = idx.min(self.hunks.len() - 1);
        let start = self.hunks[i];
        let end = self.hunks.get(i + 1).copied().unwrap_or(self.rows.len());
        start..end
    }

    pub fn nav(&mut self, next: bool) {
        if self.hunks.is_empty() {
            return;
        }
        let n = self.hunks.len();
        self.hunk_cur = if next {
            (self.hunk_cur + 1) % n
        } else {
            (self.hunk_cur + n - 1) % n
        };
        self.scroll_to = Some(self.hunks[self.hunk_cur]);
    }
}

fn read_lines(p: &std::path::Path) -> Result<Vec<String>, String> {
    let bytes = std::fs::read(p).map_err(|e| format!("{}: {}", p.display(), e))?;
    let (text, _, _) = crate::util::decode_text(&bytes);
    Ok(text.lines().map(|s| s.to_string()).collect())
}

/// Menu / palette entry: pick two files, then open the compare view.
pub fn open_dialog(app: &mut App) {
    let Some(a) = rfd::FileDialog::new()
        .set_title("Compare — pick the FIRST (older) file")
        .pick_file()
    else {
        return;
    };
    let Some(b) = rfd::FileDialog::new()
        .set_title("Compare — pick the SECOND (newer) file")
        .pick_file()
    else {
        return;
    };
    open_paths(app, a, b);
}

pub fn open_paths(app: &mut App, a: PathBuf, b: PathBuf) {
    match (read_lines(&a), read_lines(&b)) {
        (Ok(l), Ok(r)) => {
            if l.len() > MAX_LINES || r.len() > MAX_LINES {
                app.set_status(format!("File too large to compare (max {} lines)", MAX_LINES));
                return;
            }
            let steps_budget = l.len() + r.len();
            let _ = steps_budget;
            app.diff = Some(DiffView::build(a, l, b, r, false));
            app.set_status("Comparing — F8 / Shift+F8 jump between hunks, Esc closes");
        }
        (Err(e), _) | (_, Err(e)) => app.set_status(format!("Compare failed: {}", e)),
    }
}

/// Re-read both files from disk (header button).
pub fn reload(app: &mut App) {
    if let Some(v) = app.diff.take() {
        let (a, b) = (v.left_path.clone(), v.right_path.clone());
        drop(v);
        open_paths(app, a, b);
    }
}

// ---------------------------------------------------------------- UI

const GUTTER: f32 = 46.0;

pub fn show(app: &mut App, ctx: &egui::Context) {
    let t = app.theme_now.clone();
    let fs = app.editor_font_size();
    let line_h = fs * 1.35;

    // Esc closes (unless some other modal is up)
    if app.palette.is_none()
        && !app.about_open
        && app.input_dialog.is_none()
        && app.github.is_none()
        && !app.exit_confirm
        && app.pending_close.is_none()
        && !app.stats_open
        && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
    {
        app.diff = None;
        return;
    }

    let mut view = match app.diff.take() {
        Some(v) => v,
        None => return,
    };
    let mut close = false;

    egui::CentralPanel::default()
        .frame(egui::Frame::none().fill(t.editor_bg))
        .show(ctx, |ui| {
            // ---- header ------------------------------------------------
            ui.horizontal(|ui| {
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(format!("● {}", view.left_name))
                        .size(12.0)
                        .color(t.err),
                );
                ui.label(egui::RichText::new("⇄").size(12.0).color(t.text_faint));
                ui.label(
                    egui::RichText::new(format!("● {}", view.right_name))
                        .size(12.0)
                        .color(t.ok),
                );
                ui.label(
                    egui::RichText::new(format!("+{} −{}", view.ins_count, view.del_count))
                        .size(11.0)
                        .color(t.text_dim),
                );
                if view.used_fallback {
                    ui.label(
                        egui::RichText::new("(approximate)")
                            .size(11.0)
                            .color(t.warn),
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("✕ Close").clicked() {
                        close = true;
                    }
                    if ui.button("↻ Reload").clicked() {
                        reload(app);
                    }
                    let n = view.hunks.len();
                    if ui
                        .button(format!("{} / {} ›", if n == 0 { 0 } else { view.hunk_cur + 1 }, n))
                        .clicked()
                    {
                        view.nav(true);
                    }
                    if ui.button("‹").clicked() {
                        view.nav(false);
                    }
                    ui.label(
                        egui::RichText::new("F8 / Shift+F8 · Esc")
                            .size(10.5)
                            .color(t.text_faint),
                    );
                });
            });
            ui.separator();

            // ---- rows ---------------------------------------------------
            let mono = egui::FontId::monospace(fs);
            let numf = egui::FontId::monospace((fs * 0.82).max(8.0));
            let n_rows = view.rows.len();
            let cur_hunk = view.hunk_range(view.hunk_cur);
            // pending hunk jump → absolute content-space Y offset (row * step)
            let step = line_h + ui.spacing().item_spacing.y;
            let avail = ui.available_height();
            let jump_y = view
                .scroll_to
                .take()
                .map(|r| ((r as f32 * step) - avail * 0.4).max(0.0));
            let mut sa = egui::ScrollArea::vertical()
                .id_salt("rea_diff")
                .auto_shrink([false, false]);
            if let Some(y) = jump_y {
                sa = sa.scroll_offset(egui::Vec2::new(0.0, y));
            }
            sa.show_rows(ui, line_h, n_rows.max(1), |ui, range| {
                let half = ((ui.available_width() - 2.0 * GUTTER) / 2.0).max(60.0);
                    for row_idx in range {
                        let row = view.rows[row_idx];
                        let (rect, _) = ui
                            .allocate_exact_size(
                                egui::vec2(ui.available_width(), line_h),
                                egui::Sense::hover(),
                            )
                            ;
                        let p = ui.painter_at(rect);

                        // backgrounds
                        let (bg_l, bg_r) = match row.kind {
                            RowKind::Eq => (None, None),
                            RowKind::Del => (Some(t.err.gamma_multiply(0.20)), Some(t.err.gamma_multiply(0.07))),
                            RowKind::Ins => (Some(t.ok.gamma_multiply(0.07)), Some(t.ok.gamma_multiply(0.20))),
                            RowKind::Mod => (
                                Some(t.warn.gamma_multiply(0.22)),
                                Some(t.warn.gamma_multiply(0.14)),
                            ),
                        };
                        let mid = GUTTER + half;
                        if let Some(c) = bg_l {
                            p.rect_filled(egui::Rect::from_min_max(
                                egui::pos2(rect.left(), rect.top()),
                                egui::pos2(rect.left() + mid, rect.bottom()),
                            ), 0.0, c);
                        }
                        if let Some(c) = bg_r {
                            p.rect_filled(egui::Rect::from_min_max(
                                egui::pos2(rect.left() + mid, rect.top()),
                                egui::pos2(rect.right(), rect.bottom()),
                            ), 0.0, c);
                        }
                        // current-hunk marker
                        if cur_hunk.contains(&row_idx) {
                            p.line_segment(
                                [egui::pos2(rect.left() + 1.5, rect.top()), egui::pos2(rect.left() + 1.5, rect.bottom())],
                                egui::Stroke::new(2.0, t.accent),
                            );
                        }
                        // column separators
                        p.line_segment(
                            [egui::pos2(rect.left() + GUTTER, rect.top()), egui::pos2(rect.left() + GUTTER, rect.bottom())],
                            egui::Stroke::new(1.0, t.border.gamma_multiply(0.5)),
                        );
                        p.line_segment(
                            [egui::pos2(rect.left() + mid, rect.top()), egui::pos2(rect.left() + mid, rect.bottom())],
                            egui::Stroke::new(1.0, t.border.gamma_multiply(0.5)),
                        );

                        // left cell
                        if let Some(li) = row.l {
                            let x0 = rect.left() + 4.0;
                            p.text(
                                egui::pos2(x0, rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                format!("{:>4}", li + 1),
                                numf.clone(),
                                t.text_faint,
                            );
                            let line = view.left.get(li).map(|s| s.as_str()).unwrap_or("");
                            p.text(
                                egui::pos2(rect.left() + GUTTER + 4.0, rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                line,
                                mono.clone(),
                                if row.kind == RowKind::Eq { t.text } else { t.text },
                            );
                        }
                        // right cell
                        if let Some(ri) = row.r {
                            let x0 = rect.left() + mid + 4.0;
                            p.text(
                                egui::pos2(x0, rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                format!("{:>4}", ri + 1),
                                numf.clone(),
                                t.text_faint,
                            );
                            let line = view.right.get(ri).map(|s| s.as_str()).unwrap_or("");
                            p.text(
                                egui::pos2(rect.left() + mid + GUTTER + 4.0, rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                line,
                                mono.clone(),
                                t.text,
                            );
                        }
                    }
                });
        });

    if close {
        app.diff = None;
    } else {
        app.diff = Some(view);
    }
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn identical_files_all_eq() {
        let a = lines(&["a", "b", "c"]);
        let rows = compute_diff(&a, &a);
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|r| r.kind == RowKind::Eq));
    }

    #[test]
    fn pure_insert() {
        let a = lines(&["a", "d"]);
        let b = lines(&["a", "b", "c", "d"]);
        let rows = compute_diff(&a, &b);
        let ins: Vec<_> = rows.iter().filter(|r| r.kind == RowKind::Ins).collect();
        assert_eq!(ins.len(), 2);
        assert_eq!(ins[0].r, Some(1));
        assert_eq!(ins[1].r, Some(2));
        assert_eq!(rows.iter().filter(|r| r.kind == RowKind::Del).count(), 0);
    }

    #[test]
    fn pure_delete() {
        let a = lines(&["a", "x", "y", "d"]);
        let b = lines(&["a", "d"]);
        let rows = compute_diff(&a, &b);
        assert_eq!(rows.iter().filter(|r| r.kind == RowKind::Del).count(), 2);
        assert_eq!(rows.iter().filter(|r| r.kind == RowKind::Ins).count(), 0);
    }

    #[test]
    fn modify_becomes_mod_rows() {
        let a = lines(&["old1", "old2", "keep"]);
        let b = lines(&["new1", "new2", "keep"]);
        let rows = compute_diff(&a, &b);
        let mods: Vec<_> = rows.iter().filter(|r| r.kind == RowKind::Mod).collect();
        assert_eq!(mods.len(), 2);
        assert_eq!(mods[0].l, Some(0));
        assert_eq!(mods[0].r, Some(0));
        assert_eq!(mods[1].l, Some(1));
        assert_eq!(mods[1].r, Some(1));
    }

    #[test]
    fn empty_vs_text_all_ins() {
        let b = lines(&["one", "two"]);
        let rows = compute_diff(&[], &b);
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.kind == RowKind::Ins));
    }

    #[test]
    fn hunks_detected_and_nav_wraps() {
        let a = lines(&["a", "B", "c", "d", "E", "f"]);
        let b = lines(&["a", "b", "c", "d", "e", "f"]);
        let v = DiffView::build(PathBuf::from("a"), a, PathBuf::from("b"), b, false);
        assert_eq!(v.hunks.len(), 2);
        let mut v2 = DiffView::build(PathBuf::from("a"), lines(&["x"]), PathBuf::from("b"), lines(&["y"]), false);
        assert_eq!(v2.hunks.len(), 1);
        v2.nav(true);
        assert_eq!(v2.hunk_cur, 0); // wraps
        assert_eq!(v2.scroll_to, Some(0));
    }

    #[test]
    fn fallback_pairs_rest() {
        let a = lines(&["1", "2", "3", "4"]);
        let b = lines(&["1", "9", "9", "9", "5"]);
        let steps = fallback_steps(&a, &b);
        let rows = rows_from_steps(&steps);
        assert_eq!(rows[0].kind, RowKind::Eq);
        assert_eq!(rows[1].kind, RowKind::Mod);
        assert_eq!(rows[2].kind, RowKind::Mod);
        assert_eq!(rows[3].kind, RowKind::Mod);
        assert_eq!(rows[4].kind, RowKind::Ins); // leftover right line (5 rows total)
        assert_eq!(rows.len(), 5);
    }

    #[test]
    fn myers_matches_fallback_on_simple() {
        // sanity: Myers result for a small case equals the expected row count
        let a = lines(&["a", "b", "c"]);
        let b = lines(&["a", "X", "c"]);
        let steps = myers_steps(&a, &b).expect("within budget");
        let rows = rows_from_steps(&steps);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1].kind, RowKind::Mod);
    }
}
