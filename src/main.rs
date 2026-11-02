#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod bars;
mod colsel;
mod complete;
mod conpty;
mod dap;
mod diffview;
mod editor;
mod editops;
mod extensions;
mod formatter;
mod gitpanel;
mod github;
mod highlight;
mod icons;
mod liveserver;
mod lsp;
mod macros;
mod outline;
mod overlay;
mod plugins;
mod pty;
mod recco;
mod restclient;
mod settingsview;
mod terminal;
mod texttools;
mod theme;
mod todopanel;
mod undo;
mod util;
mod vt;
mod wssearch;

use eframe::egui;

/// Decode assets/icon48.bmp (32-bit BGRA BMP) by hand into egui::IconData.
/// Applied at runtime so the title bar AND taskbar always show the REA icon,
/// even if the exe resource is unavailable. No image crate needed.
fn load_icon() -> Option<egui::IconData> {
    const BMP: &[u8] = include_bytes!("../assets/icon48.bmp");
    if BMP.len() < 54 || &BMP[0..2] != b"BM" {
        return None;
    }
    let px_off = u32::from_le_bytes(BMP[10..14].try_into().ok()?) as usize;
    let w = i32::from_le_bytes(BMP[18..22].try_into().ok()?);
    let h = i32::from_le_bytes(BMP[22..26].try_into().ok()?);
    let bpp = u16::from_le_bytes(BMP[28..30].try_into().ok()?);
    if bpp != 32 {
        return None;
    }
    let top_down = h < 0;
    let (w, h) = (w.unsigned_abs() as usize, h.unsigned_abs() as usize);
    let row_sz = w * 4; // 32-bit rows are already 4-byte aligned
    let mut rgba = vec![0u8; w * h * 4];
    for row in 0..h {
        let src_row = if top_down { row } else { h - 1 - row };
        let src = px_off + src_row * row_sz;
        if src + row_sz > BMP.len() {
            return None;
        }
        for x in 0..w {
            let s = src + x * 4;
            let d = (row * w + x) * 4;
            rgba[d] = BMP[s + 2]; // B -> R
            rgba[d + 1] = BMP[s + 1];
            rgba[d + 2] = BMP[s]; // R -> B
            rgba[d + 3] = BMP[s + 3];
        }
    }
    Some(egui::IconData {
        width: w as u32,
        height: h as u32,
        rgba,
    })
}

fn main() -> eframe::Result<()> {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("REA Studio")
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([940.0, 560.0]);
    if let Some(icon) = load_icon() {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "REA Studio",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
