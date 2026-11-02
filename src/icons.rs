use eframe::egui;
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};

/// ------- file-type mini badges, VS Code style (60+ extensions) -------
///
/// Each extension gets a small colored chip with a short glyph/label, like the
/// file icons in the VS Code explorer. Unknown extensions get their first 3
/// letters on a neutral chip, so EVERY file type has an icon.

pub fn ext_meta(ext: &str) -> (String, Color32, bool) {
    // (label, chip color, light chip -> dark text)
    match ext {
        // --- languages the user asked for + the big ones ---
        "js" | "mjs" | "cjs" => ("JS".into(), c("#f7df1e"), true),
        "jsx" => ("JSX".into(), c("#61dafb"), false),
        "ts" => ("TS".into(), c("#3178c6"), false),
        "tsx" => ("TSX".into(), c("#3178c6"), false),
        "py" | "pyw" => ("PY".into(), c("#3776ab"), false),
        "rs" => ("RS".into(), c("#ce7a58"), false),
        "html" | "htm" => ("</>".into(), c("#e34c26"), false),
        "css" => ("CSS".into(), c("#264de4"), false),
        "scss" | "sass" => ("SCS".into(), c("#c6538c"), false),
        "less" => ("LES".into(), c("#2f6baa"), false),
        "go" => ("GO".into(), c("#00add8"), false),
        "bak" | "old" | "orig" | "backup" => ("BAK".into(), c("#757575"), false),
        "c" => ("C".into(), c("#555555"), false),
        "h" => ("H".into(), c("#519aba"), false),
        "cpp" | "cc" | "cxx" => ("C++".into(), c("#f34b7d"), false),
        "hpp" | "hh" => ("HPP".into(), c("#519aba"), false),
        "cs" => ("CS".into(), c("#178600"), false),
        "java" => ("JAV".into(), c("#b07219"), false),
        "kt" | "kts" => ("KT".into(), c("#a97bff"), false),
        "swift" => ("SW".into(), c("#f05138"), false),
        "dart" => ("DA".into(), c("#00b4ab"), false),
        "php" => ("PHP".into(), c("#4f5d95"), false),
        "rb" => ("RB".into(), c("#cc342d"), false),
        "lua" => ("LUA".into(), c("#2c64ae"), false),
        "pl" | "pm" => ("PL".into(), c("#7b5cc4"), false),
        "r" => ("R".into(), c("#276dc3"), false),
        "scala" => ("SC".into(), c("#dc322f"), false),
        "hs" => ("HS".into(), c("#5e5086"), false),
        "ml" | "mli" => ("ML".into(), c("#ef7a08"), false),
        "clj" | "cljs" => ("CLJ".into(), c("#5888bd"), false),
        "ex" | "exs" => ("EX".into(), c("#6e4a7e"), false),
        "erl" | "hrl" => ("ERL".into(), c("#b83983"), false),
        "fs" | "fsi" | "fsx" => ("FS".into(), c("#378bba"), false),
        "groovy" => ("GRV".into(), c("#4298b8"), false),
        "vb" => ("VB".into(), c("#945db7"), false),
        "pas" | "pp" | "dpr" => ("PAS".into(), c("#7b8dd4"), false),
        "f90" | "f95" | "for" => ("F90".into(), c("#4d41b1"), false),
        "d" => ("D".into(), c("#ba595e"), false),
        "cob" | "cobol" | "cbl" => ("COB".into(), c("#8b5cf6"), false),
        "asm" | "nasm" | "s" => ("ASM".into(), c("#6b7280"), false),
        "zig" => ("ZIG".into(), c("#f7a41d"), true),
        "nim" => ("NIM".into(), c("#ffe953"), true),
        "jl" => ("JL".into(), c("#9558b2"), false),
        "cr" => ("CR".into(), c("#4d4d4d"), false),
        "coffee" => ("COF".into(), c("#244776"), false),
        "vue" => ("VUE".into(), c("#41b883"), false),
        "svelte" => ("SVE".into(), c("#ff3e00"), false),
        "haml" => ("HAM".into(), c("#d6c387"), true),
        "vim" => ("VIM".into(), c("#019733"), false),

        // --- shells / build ---
        "sh" | "bash" | "zsh" => ("SH".into(), c("#89e051"), true),
        "fish" => ("FSH".into(), c("#89e051"), true),
        "ps1" | "psm1" => ("PS1".into(), c("#3f7fc1"), false),
        "bat" | "cmd" => ("BAT".into(), c("#c1f12e"), true),
        "mk" | "mak" | "make" => ("MK".into(), c("#427819"), false),
        "cmake" => ("CMK".into(), c("#064f8c"), false),
        "gradle" => ("GRD".into(), c("#02303a"), false),
        "dockerfile" => ("DOC".into(), c("#2496ed"), false),

        // --- data / markup / config ---
        "json" => ("{}".into(), c("#cbcb41"), true),
        "xml" | "xsl" | "plist" => ("XML".into(), c("#4a9de8"), false),
        "svg" => ("SVG".into(), c("#ffb13b"), true),
        "yml" | "yaml" => ("YML".into(), c("#f0ad4e"), true),
        "toml" => ("TOML".into(), c("#9c4221"), false),
        "ini" | "cfg" | "conf" | "properties" | "env" => ("INI".into(), c("#6d8086"), false),
        "md" | "markdown" => ("MD".into(), c("#519aba"), false),
        "txt" | "text" => ("TXT".into(), c("#9aa0a6"), false),
        "log" => ("LOG".into(), c("#8d8d8d"), false),
        "csv" | "tsv" => ("CSV".into(), c("#237346"), false),
        "tex" | "sty" | "cls" => ("TEX".into(), c("#7ca30c"), true),
        "diff" | "patch" => ("DIF".into(), c("#6d8086"), false),
        "sql" => ("SQL".into(), c("#da7b39"), false),
        "graphql" | "gql" => ("GQL".into(), c("#e10098"), false),
        "proto" => ("PRT".into(), c("#4285f4"), false),
        "lock" => ("LCK".into(), c("#8d6e63"), false),

        // --- binaries / archives / media / docs ---
        "exe" | "dll" | "so" | "dylib" | "bin" | "dat" | "iso" => {
            ("BIN".into(), c("#7e57c2"), false)
        }
        "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "rar" => ("ZIP".into(), c("#a1887f"), false),
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "ico" | "tiff" => {
            ("IMG".into(), c("#ab47bc"), false)
        }
        "mp3" | "wav" | "flac" | "ogg" | "m4a" => ("AUD".into(), c("#f06292"), false),
        "mp4" | "avi" | "mkv" | "mov" | "webm" => ("VID".into(), c("#ba68c8"), false),
        "pdf" => ("PDF".into(), c("#d93831"), false),
        "db" | "sqlite" | "sqlite3" | "mdb" => ("DB".into(), c("#ffa726"), true),
        "ttf" | "otf" | "woff" | "woff2" => ("FNT".into(), c("#26a69a"), false),
        _ => {
            let l: String = ext
                .chars()
                .take(3)
                .collect::<String>()
                .to_uppercase();
            if l.is_empty() {
                ("•".into(), c("#808080"), false)
            } else {
                (l, c("#808080"), false)
            }
        }
    }
}

fn c(hex: &str) -> Color32 {
    Color32::from_hex(hex).unwrap_or(Color32::GRAY)
}

fn lum(col: Color32) -> f32 {
    (0.299 * col.r() as f32 + 0.587 * col.g() as f32 + 0.114 * col.b() as f32) / 255.0
}

/// Draw a small file-type badge (20x16) at the current layout position.
pub fn badge(ui: &mut egui::Ui, ext: &str) -> egui::Response {
    let (lbl, col, light) = ext_meta(ext);
    let (rect, rsp) = ui.allocate_exact_size(Vec2::new(20.0, 16.0), Sense::hover());
    ui.painter().rect_filled(rect, 4.0, col);
    let tcol = if light { Color32::from_rgb(35, 35, 35) } else { Color32::WHITE };
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        lbl,
        FontId::proportional(8.5),
        tcol,
    );
    rsp
}

/// ------- activity-bar / panel icons (hand-drawn with the Painter — crisp & theme aware) -------

#[derive(Clone, Copy, PartialEq)]
pub enum Act {
    Explorer,
    Search,
    Extensions,
    Terminal,
    Theme,
    Info,
    Settings,
    /// v1.9: function list (outline) panel
    Outline,
    /// v2.1: workspace TODO scanner panel
    Todo,
}

pub fn activity(ui: &mut egui::Ui, icon: Act, color: Color32) -> egui::Response {
    let (rect, rsp) = ui.allocate_exact_size(Vec2::new(30.0, 30.0), Sense::click());
    let p = ui.painter_at(rect);
    let c = rect.center();
    let sw = Stroke::new(1.5, color);
    match icon {
        Act::Explorer => {
            let r = Rect::from_min_size(c + Vec2::new(-6.0, -8.0), Vec2::new(12.0, 16.0));
            p.rect_stroke(r, 2.0, sw);
            p.line_segment([c + Vec2::new(3.0, -8.0), c + Vec2::new(6.0, -5.0)], sw);
            p.line_segment([c + Vec2::new(3.0, -8.0), c + Vec2::new(3.0, -5.0)], sw);
            p.line_segment([c + Vec2::new(3.0, -5.0), c + Vec2::new(6.0, -5.0)], sw);
        }
        Act::Search => {
            p.circle_stroke(c + Vec2::new(-1.5, -1.5), 5.5, sw);
            p.line_segment([c + Vec2::new(3.0, 3.0), c + Vec2::new(7.5, 7.5)], sw);
        }
        Act::Extensions => {
            for d in [
                Vec2::new(-8.0, -8.0),
                Vec2::new(1.0, -8.0),
                Vec2::new(-8.0, 1.0),
                Vec2::new(1.0, 1.0),
            ] {
                p.rect_filled(Rect::from_min_size(c + d, Vec2::new(7.0, 7.0)), 1.5, color);
            }
        }
        Act::Terminal => {
            p.line_segment([c + Vec2::new(-7.0, -4.0), c + Vec2::new(-2.5, 0.0)], sw);
            p.line_segment([c + Vec2::new(-2.5, 0.0), c + Vec2::new(-7.0, 4.0)], sw);
            p.line_segment([c + Vec2::new(0.5, 4.0), c + Vec2::new(7.0, 4.0)], sw);
        }
        Act::Theme => {
            p.circle_stroke(c, 7.0, sw);
            p.circle_filled(c + Vec2::new(2.0, -2.0), 3.0, color);
        }
        Act::Info => {
            p.circle_stroke(c, 7.0, sw);
            p.text(
                c + Vec2::new(0.0, 0.5),
                Align2::CENTER_CENTER,
                "i",
                FontId::proportional(10.0),
                color,
            );
        }
        Act::Settings => {
            // gear: outer circle + teeth
            p.circle_stroke(c, 4.5, sw);
            for k in 0..8 {
                let a = k as f32 * std::f32::consts::TAU / 8.0;
                let dir = Vec2::new(a.cos(), a.sin());
                p.line_segment(
                    [c + dir * 5.5, c + dir * 8.0],
                    Stroke::new(1.8, color),
                );
            }
        }
        Act::Outline => {
            // { } braces with a middle line — the "symbols" glyph
            p.line_segment([c + Vec2::new(-3.0, -8.0), c + Vec2::new(-6.5, -8.0)], sw);
            p.line_segment([c + Vec2::new(-6.5, -8.0), c + Vec2::new(-6.5, -2.0)], sw);
            p.line_segment([c + Vec2::new(-6.5, -2.0), c + Vec2::new(-3.0, 0.0)], sw);
            p.line_segment([c + Vec2::new(-3.0, 0.0), c + Vec2::new(-6.5, 2.0)], sw);
            p.line_segment([c + Vec2::new(-6.5, 2.0), c + Vec2::new(-6.5, 8.0)], sw);
            p.line_segment([c + Vec2::new(-6.5, 8.0), c + Vec2::new(-3.0, 8.0)], sw);
            p.line_segment([c + Vec2::new(3.0, -8.0), c + Vec2::new(6.5, -8.0)], sw);
            p.line_segment([c + Vec2::new(6.5, -8.0), c + Vec2::new(6.5, -2.0)], sw);
            p.line_segment([c + Vec2::new(6.5, -2.0), c + Vec2::new(3.0, 0.0)], sw);
            p.line_segment([c + Vec2::new(3.0, 0.0), c + Vec2::new(6.5, 2.0)], sw);
            p.line_segment([c + Vec2::new(6.5, 2.0), c + Vec2::new(6.5, 8.0)], sw);
            p.line_segment([c + Vec2::new(6.5, 8.0), c + Vec2::new(3.0, 8.0)], sw);
            p.line_segment([c + Vec2::new(-2.0, 0.0), c + Vec2::new(2.0, 0.0)], sw);
        }
        Act::Todo => {
            // checkbox with a check mark
            let r = Rect::from_min_size(c + Vec2::new(-6.5, -6.5), Vec2::new(13.0, 13.0));
            p.rect_stroke(r, 2.5, sw);
            p.line_segment([c + Vec2::new(-3.5, 0.0), c + Vec2::new(-1.0, 3.0)], sw);
            p.line_segment([c + Vec2::new(-1.0, 3.0), c + Vec2::new(4.0, -3.5)], sw);
        }
    }
    rsp
}

/// Small chevron (for collapsible tree rows).
pub fn chevron(p: &egui::Painter, center: Pos2, right: bool, col: Color32) {
    let dx = if right { 1.0 } else { -1.0 };
    let sw = Stroke::new(1.4, col);
    p.line_segment([center + Vec2::new(-2.0 * dx, -3.0), center + Vec2::new(2.0 * dx, 0.0)], sw);
    p.line_segment([center + Vec2::new(2.0 * dx, 0.0), center + Vec2::new(-2.0 * dx, 3.0)], sw);
}

/// Folder glyph.
pub fn folder(p: &egui::Painter, center: Pos2, col: Color32) {
    let r = Rect::from_min_size(center + Vec2::new(-6.0, -4.0), Vec2::new(12.0, 8.5));
    p.rect_stroke(r, 2.0, Stroke::new(1.3, col));
    p.line_segment(
        [center + Vec2::new(-6.0, -2.2), center + Vec2::new(-2.5, -2.2)],
        Stroke::new(1.3, col),
    );
}

/// Small "x" close icon.
pub fn close_x(p: &egui::Painter, center: Pos2, col: Color32) {
    let sw = Stroke::new(1.4, col);
    p.line_segment([center + Vec2::new(-3.0, -3.0), center + Vec2::new(3.0, 3.0)], sw);
    p.line_segment([center + Vec2::new(3.0, -3.0), center + Vec2::new(-3.0, 3.0)], sw);
}

/// Magnifier for the find box.
pub fn magnifier(p: &egui::Painter, center: Pos2, col: Color32) {
    let sw = Stroke::new(1.5, col);
    p.circle_stroke(center + Vec2::new(-1.0, -1.0), 4.0, sw);
    p.line_segment([center + Vec2::new(1.8, 1.8), center + Vec2::new(5.0, 5.0)], sw);
}

/// Left / right arrow chevrons for find navigation.
pub fn arrow(p: &egui::Painter, center: Pos2, right: bool, col: Color32) {
    chevron(p, center, right, col);
}
