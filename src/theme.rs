use eframe::egui;
use egui::{Color32, Rounding, Stroke, Vec2};

/// A complete color theme. All fields are animatable (lerped) colors.
#[derive(Clone, Debug)]
pub struct Theme {
    pub name: String,
    pub dark: bool,
    pub bg: Color32,          // window / popup fill
    pub panel: Color32,       // side panel (explorer / extensions)
    pub panel2: Color32,      // activity bar + status bar
    pub editor_bg: Color32,
    pub gutter_bg: Color32,
    pub tab_bg: Color32,
    pub tab_active_bg: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_faint: Color32,
    pub accent: Color32,
    pub accent_text: Color32,
    pub border: Color32,
    pub hover: Color32,
    pub widget: Color32,
    pub active_row: Color32,
    pub find_bg: Color32,
    pub shadow_col: Color32,
    pub ok: Color32,
    pub warn: Color32,
    pub err: Color32,
}

pub fn dark() -> Theme {
    Theme {
        name: "Dark+".into(),
        dark: true,
        bg: c("#1f1f1f"),
        panel: c("#252526"),
        panel2: c("#333333"),
        editor_bg: c("#1e1e1e"),
        gutter_bg: c("#1e1e1e"),
        tab_bg: c("#2d2d2d"),
        tab_active_bg: c("#1e1e1e"),
        text: c("#d4d4d4"),
        text_dim: c("#9d9d9d"),
        text_faint: c("#6e6e6e"),
        accent: c("#0a7eca"),
        accent_text: c("#ffffff"),
        border: c("#3c3c3c"),
        hover: c("#2a2d2e"),
        widget: c("#37373d"),
        active_row: c("#264f78"),
        find_bg: c("#252526"),
        shadow_col: Color32::from_black_alpha(150),
        ok: c("#4ec9b0"),
        warn: c("#dcdcaa"),
        err: c("#f14c4c"),
    }
}

pub fn light() -> Theme {
    Theme {
        name: "Google Light".into(),
        dark: false,
        bg: c("#ffffff"),
        panel: c("#f8f9fa"),
        panel2: c("#f1f3f4"),
        editor_bg: c("#ffffff"),
        gutter_bg: c("#ffffff"),
        tab_bg: c("#f1f3f4"),
        tab_active_bg: c("#ffffff"),
        text: c("#202124"),
        text_dim: c("#5f6368"),
        text_faint: c("#9aa0a6"),
        accent: c("#1a73e8"),
        accent_text: c("#ffffff"),
        border: c("#dadce0"),
        hover: c("#f1f3f4"),
        widget: c("#ffffff"),
        active_row: c("#e8f0fe"),
        find_bg: c("#ffffff"),
        shadow_col: Color32::from_black_alpha(60),
        ok: c("#188038"),
        warn: c("#ea8600"),
        err: c("#d93025"),
    }
}

fn c(hex: &str) -> Color32 {
    Color32::from_hex(hex).unwrap_or(Color32::GRAY)
}

fn lerp32(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| -> u8 { (x as f32 + (y as f32 - x as f32) * t).round() as u8 };
    Color32::from_rgba_unmultiplied(
        f(a.r(), b.r()),
        f(a.g(), b.g()),
        f(a.b(), b.b()),
        f(a.a(), b.a()),
    )
}

/// Smoothly interpolate two themes (pure Rust, no shaders — fast).
pub fn lerp(a: &Theme, b: &Theme, t: f32) -> Theme {
    Theme {
        name: b.name.clone(),
        dark: b.dark,
        bg: lerp32(a.bg, b.bg, t),
        panel: lerp32(a.panel, b.panel, t),
        panel2: lerp32(a.panel2, b.panel2, t),
        editor_bg: lerp32(a.editor_bg, b.editor_bg, t),
        gutter_bg: lerp32(a.gutter_bg, b.gutter_bg, t),
        tab_bg: lerp32(a.tab_bg, b.tab_bg, t),
        tab_active_bg: lerp32(a.tab_active_bg, b.tab_active_bg, t),
        text: lerp32(a.text, b.text, t),
        text_dim: lerp32(a.text_dim, b.text_dim, t),
        text_faint: lerp32(a.text_faint, b.text_faint, t),
        accent: lerp32(a.accent, b.accent, t),
        accent_text: lerp32(a.accent_text, b.accent_text, t),
        border: lerp32(a.border, b.border, t),
        hover: lerp32(a.hover, b.hover, t),
        widget: lerp32(a.widget, b.widget, t),
        active_row: lerp32(a.active_row, b.active_row, t),
        find_bg: lerp32(a.find_bg, b.find_bg, t),
        shadow_col: lerp32(a.shadow_col, b.shadow_col, t),
        ok: lerp32(a.ok, b.ok, t),
        warn: lerp32(a.warn, b.warn, t),
        err: lerp32(a.err, b.err, t),
    }
}

/// Blend two colors (used to derive secondary theme colors).
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    lerp32(a, b, t)
}

/// Perceived luminance 0.0 (black) .. 1.0 (white).
pub fn lum(c: Color32) -> f32 {
    (0.299 * c.r() as f32 + 0.587 * c.g() as f32 + 0.114 * c.b() as f32) / 255.0
}

fn hex(c: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

/// Serialize a Theme back to the .json format understood by from_json,
/// so built-in extension themes can be written to the user's themes folder.
pub fn to_json(t: &Theme) -> String {
    format!(
        r#"{{
  "name": "{}",
  "dark": {},
  "bg": "{}",
  "panel": "{}",
  "panel2": "{}",
  "editor_bg": "{}",
  "gutter_bg": "{}",
  "tab_bg": "{}",
  "tab_active_bg": "{}",
  "text": "{}",
  "text_dim": "{}",
  "text_faint": "{}",
  "accent": "{}",
  "accent_text": "{}",
  "border": "{}",
  "hover": "{}",
  "widget": "{}",
  "active_row": "{}",
  "find_bg": "{}"
}}"#,
        t.name,
        t.dark,
        hex(t.bg),
        hex(t.panel),
        hex(t.panel2),
        hex(t.editor_bg),
        hex(t.gutter_bg),
        hex(t.tab_bg),
        hex(t.tab_active_bg),
        hex(t.text),
        hex(t.text_dim),
        hex(t.text_faint),
        hex(t.accent),
        hex(t.accent_text),
        hex(t.border),
        hex(t.hover),
        hex(t.widget),
        hex(t.active_row),
        hex(t.find_bg),
    )
}

fn wv(
    bg: Color32,
    weak: Color32,
    stroke: Color32,
    fg: Color32,
) -> egui::style::WidgetVisuals {
    egui::style::WidgetVisuals {
        bg_fill: bg,
        weak_bg_fill: weak,
        bg_stroke: Stroke::new(1.0, stroke),
        fg_stroke: Stroke::new(1.0, fg),
        rounding: Rounding::same(6.0),
        expansion: 0.0,
    }
}

/// Build egui Visuals (including soft shadows everywhere) from a Theme.
pub fn visuals(t: &Theme) -> egui::Visuals {
    let mut v = egui::Visuals::dark();
    v.window_fill = t.bg;
    v.panel_fill = t.panel;
    v.extreme_bg_color = t.editor_bg;
    v.faint_bg_color = t.hover;
    v.override_text_color = Some(t.text);

    v.widgets.noninteractive = wv(t.panel, t.panel, t.border, t.text_dim);
    v.widgets.inactive = wv(t.widget, t.widget, t.border, t.text);
    v.widgets.hovered = wv(t.hover, t.hover, t.accent, t.text);
    v.widgets.active = wv(t.active_row, t.active_row, t.accent, t.text);
    v.widgets.open = wv(t.hover, t.hover, t.accent, t.text);

    v.selection.bg_fill = t.active_row;
    v.selection.stroke = Stroke::new(1.0, t.accent);

    v.window_rounding = Rounding::same(10.0);
    v.menu_rounding = Rounding::same(8.0);
    v.window_shadow = egui::Shadow {
        offset: Vec2::new(0.0, 8.0),
        blur: 24.0,
        spread: 0.0,
        color: t.shadow_col,
    };
    v.popup_shadow = egui::Shadow {
        offset: Vec2::new(0.0, 4.0),
        blur: 14.0,
        spread: 0.0,
        color: t.shadow_col,
    };
    v
}

/// Apply theme to context (visuals only; fonts are managed separately).
pub fn apply(ctx: &egui::Context, t: &Theme) {
    ctx.set_visuals(visuals(t));
}

// ---------- user theme file (.json) support ----------

/// Parse a user theme JSON, filling missing colors from a base theme.
pub fn from_json(json: &str, base: &Theme) -> Result<Theme, String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("invalid JSON: {e}"))?;
    let mut th = base.clone();
    if let Some(n) = v.get("name").and_then(|x| x.as_str()) {
        th.name = n.to_string();
    }
    if let Some(d) = v.get("dark").and_then(|x| x.as_bool()) {
        th.dark = d;
    }
    let mut get = |key: &str| -> Option<Color32> {
        v.get(key)
            .and_then(|x| x.as_str())
            .and_then(|s| Color32::from_hex(s).ok())
    };
    if let Some(x) = get("bg") { th.bg = x; }
    if let Some(x) = get("panel") { th.panel = x; }
    if let Some(x) = get("panel2") { th.panel2 = x; }
    if let Some(x) = get("editor_bg") { th.editor_bg = x; }
    if let Some(x) = get("gutter_bg") { th.gutter_bg = x; }
    if let Some(x) = get("tab_bg") { th.tab_bg = x; }
    if let Some(x) = get("tab_active_bg") { th.tab_active_bg = x; }
    if let Some(x) = get("text") { th.text = x; }
    if let Some(x) = get("text_dim") { th.text_dim = x; }
    if let Some(x) = get("text_faint") { th.text_faint = x; }
    if let Some(x) = get("accent") { th.accent = x; }
    if let Some(x) = get("accent_text") { th.accent_text = x; }
    if let Some(x) = get("border") { th.border = x; }
    if let Some(x) = get("hover") { th.hover = x; }
    if let Some(x) = get("widget") { th.widget = x; }
    if let Some(x) = get("active_row") { th.active_row = x; }
    if let Some(x) = get("find_bg") { th.find_bg = x; }
    Ok(th)
}

pub const USER_THEME_EXAMPLE: &str = r##"{
  "name": "My Theme",
  "dark": true,
  "bg": "#1f1f1f",
  "panel": "#252526",
  "panel2": "#333333",
  "editor_bg": "#1e1e1e",
  "gutter_bg": "#1e1e1e",
  "tab_bg": "#2d2d2d",
  "tab_active_bg": "#1e1e1e",
  "text": "#d4d4d4",
  "text_dim": "#9d9d9d",
  "text_faint": "#6e6e6e",
  "accent": "#0a7eca",
  "accent_text": "#ffffff",
  "border": "#3c3c3c",
  "hover": "#2a2d2e",
  "widget": "#37373d",
  "active_row": "#264f78",
  "find_bg": "#252526"
}
"##;

// ---------- VS Code theme (.vsix / theme json) conversion ----------

/// Convert a VS Code color-theme JSON (workbench color tokens) into a REA
/// Theme. Fills gaps from the closest built-in base. Used by the .vsix
/// installer in the Extensions panel.
pub fn from_vscode(json: &str) -> Result<Theme, String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("invalid JSON: {e}"))?;
    let is_dark = {
        let t = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
        if !t.is_empty() {
            t.eq_ignore_ascii_case("dark")
        } else {
            // heuristics: editor.background dark?
            let bg = v
                .pointer("/colors/editor.background")
                .and_then(|x| x.as_str())
                .unwrap_or("#1e1e1e");
            let c = Color32::from_hex(bg).unwrap_or(Color32::BLACK);
            (c.r() as u32 + c.g() as u32 + c.b() as u32) < 384
        }
    };
    let base = if is_dark { dark() } else { light() };
    let dark = is_dark;
    let mut th = base.clone();

    let get = |path: &str| -> Option<Color32> {
        v.pointer(&format!("/colors/{path}"))
            .and_then(|x| x.as_str())
            .and_then(|s| parse_vscode_color(s))
    };
    let get_ui = |path: &str| -> Option<Color32> {
        // tokenColors fallback: "editor.foreground" style lookups only
        let _ = path;
        None
    };
    let _ = get_ui;

    if let Some(x) = get("editor.background") {
        th.bg = x;
        th.editor_bg = x;
        th.gutter_bg = x;
        th.tab_active_bg = x;
    }
    if let Some(x) = get("sideBar.background") {
        th.panel = x;
    }
    if let Some(x) = get("activityBar.background") {
        th.panel2 = x;
    }
    if let Some(x) = get("titleBar.activeBackground") {
        th.panel2 = x;
    }
    if let Some(x) = get("editor.foreground") {
        th.text = x;
    }
    if let Some(x) = get("descriptionForeground") {
        th.text_dim = x;
    } else {
        th.text_dim = mix(th.text, th.editor_bg, 0.35);
    }
    th.text_faint = mix(th.text_dim, th.editor_bg, 0.45);
    if let Some(x) = get("focusBorder") {
        th.accent = x;
    }
    if let Some(x) = get("activityBarBadge.background") {
        th.accent = x;
    }
    if let Some(x) = get("button.background") {
        th.accent = x;
    }
    if let Some(x) = get("list.activeSelectionBackground") {
        th.active_row = x;
    }
    if let Some(x) = get("list.hoverBackground") {
        th.hover = x;
    }
    if let Some(x) = get("editorWidget.background") {
        th.find_bg = x;
        th.widget = x;
    }
    if let Some(x) = get("tab.inactiveBackground") {
        th.tab_bg = x;
    }
    if let Some(x) = get("panel.border") {
        th.border = x;
    } else if let Some(x) = get("editorWidget.border") {
        th.border = x;
    } else {
        th.border = mix(th.panel, th.text, 0.15);
    }
    if let Some(x) = get("statusBar.background") {
        th.accent = if dark { th.accent } else { th.accent };
        let _ = x;
    }
    // status bar text: use accent text if status bar equals accent else text
    th.accent_text = if lum(th.accent) > 0.6 {
        Color32::from_rgb(25, 25, 25)
    } else {
        Color32::WHITE
    };
    if let Some(x) = get("terminal.ansiGreen") {
        th.ok = x;
    }
    if let Some(x) = get("terminal.ansiYellow") {
        th.warn = x;
    }
    if let Some(x) = get("terminal.ansiRed") {
        th.err = x;
    }
    th.shadow_col = if dark {
        Color32::from_black_alpha(150)
    } else {
        Color32::from_black_alpha(60)
    };
    th.dark = dark;
    if th.name.is_empty() || th.name == base.name {
        th.name = v
            .get("name")
            .or_else(|| v.get("title"))
            .and_then(|x| x.as_str())
            .unwrap_or("VS Code Theme")
            .to_string();
    }
    Ok(th)
}

/// VS Code allows #rgb, #rrggbb and #rrggbbaa.
fn parse_vscode_color(s: &str) -> Option<Color32> {
    let s = s.trim();
    let s = s.strip_prefix('#')?;
    match s.len() {
        3 => {
            let r = u8::from_str_radix(&s[0..1].repeat(2), 16).ok()?;
            let g = u8::from_str_radix(&s[1..2].repeat(2), 16).ok()?;
            let b = u8::from_str_radix(&s[2..3].repeat(2), 16).ok()?;
            Some(Color32::from_rgb(r, g, b))
        }
        6 => Color32::from_hex(&format!("#{s}")).ok(),
        8 => {
            let r = u8::from_str_radix(&s[0..2], 16).ok()?;
            let g = u8::from_str_radix(&s[2..4], 16).ok()?;
            let b = u8::from_str_radix(&s[4..6], 16).ok()?;
            Some(Color32::from_rgb(r, g, b)) // alpha ignored (opaque UI)
        }
        _ => None,
    }
}
