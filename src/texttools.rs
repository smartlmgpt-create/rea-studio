//! v2.1: Text tools — the Notepad++ "TextFX" counterpart.
//!
//! Line operations (sort / dedupe / reverse / remove-empty / trim), case
//! conversion, Base64 & URL encode-decode, MD5 / SHA-1 / SHA-256 hashes,
//! JSON escaping and a timestamp insert. All functions take CHAR-index
//! ranges (egui CCursor space) and are pure so they are unit-testable.
//!
//! `apply()` is the single dispatcher used by `App::tool_run()`.

use crate::util::char_slice;

// ---------------------------------------------------------------- helpers

/// Split into lines WITHOUT the trailing '\n' (also keeps a final
/// newline-less last line). `text` is always LF-normalized inside the editor.
/// A trailing '\n' does NOT produce a phantom empty line — `trailing_nl` in
/// `join_lines` restores it.
fn split_lines(text: &str) -> Vec<String> {
    let mut v: Vec<String> = text.split('\n').map(|s| s.to_string()).collect();
    if text.ends_with('\n') {
        v.pop();
    }
    v
}

/// Inverse of `split_lines`: `trailing_nl` restores the final '\n'.
fn join_lines(lines: &[String], trailing_nl: bool) -> String {
    let mut out = lines.join("\n");
    if trailing_nl {
        out.push('\n');
    }
    out
}

/// (first_line, last_line_inclusive) that a char range overlaps.
/// `None` or an empty range → the whole document is targeted.
fn line_span(text: &str, range: Option<(usize, usize)>) -> Option<(usize, usize)> {
    let (s, e) = range?;
    if e <= s {
        return None;
    }
    let l0 = crate::util::char_to_line_col(text, s).0;
    // `e` may sit right after the last selected char; step back one so a
    // selection ending exactly at a line start does not add that line.
    let e_adj = e - 1;
    let l1 = crate::util::char_to_line_col(text, e_adj).0;
    Some((l0.min(l1), l0.max(l1)))
}

fn byte_of_char(text: &str, n: usize) -> usize {
    text.char_indices().nth(n).map(|(o, _)| o).unwrap_or(text.len())
}

/// Byte range of a char range (clamped, always on char boundaries).
fn byte_range(text: &str, range: (usize, usize)) -> (usize, usize) {
    (
        byte_of_char(text, range.0),
        byte_of_char(text, range.1.max(range.0)),
    )
}

// ---------------------------------------------------------------- line ops

/// Sort the touched lines (whole doc when no selection).
fn sort_lines(text: &str, range: Option<(usize, usize)>, desc: bool) -> String {
    let trailing = text.ends_with('\n');
    let mut lines = split_lines(text);
    match line_span(text, range) {
        Some((a, b)) => {
            let b = (b + 1).min(lines.len());
            lines[a..b].sort_by(|x, y| x.cmp(y));
            if desc {
                lines[a..b].reverse();
            }
        }
        None => {
            lines.sort_by(|x, y| x.cmp(y));
            if desc {
                lines.reverse();
            }
        }
    }
    join_lines(&lines, trailing)
}

/// Remove duplicate lines inside the touched range (keeps the first
/// occurrence). Comparison is exact (case-sensitive).
fn dedupe_lines(text: &str, range: Option<(usize, usize)>) -> String {
    let trailing = text.ends_with('\n');
    let lines = split_lines(text);
    let span = match line_span(text, range) {
        Some((a, b)) => a..(b + 1).min(lines.len()),
        None => 0..lines.len(),
    };
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut kept: Vec<String> = Vec::new();
    // everything before the span is kept untouched
    let (a, b) = (span.start, span.end);
    kept.extend_from_slice(&lines[..a]);
    for l in &lines[a..b] {
        if seen.insert(l.as_str()) {
            kept.push(l.clone());
        }
    }
    kept.extend_from_slice(&lines[b..]);
    join_lines(&kept, trailing)
}

/// Remove empty (or whitespace-only) lines inside the touched range.
fn remove_empty_lines(text: &str, range: Option<(usize, usize)>) -> String {
    let trailing = text.ends_with('\n');
    let mut lines = split_lines(text);
    match line_span(text, range) {
        Some((a, b)) => {
            let b = (b + 1).min(lines.len());
            let body: Vec<String> = lines[a..b]
                .iter()
                .filter(|l| !l.trim().is_empty())
                .cloned()
                .collect();
            let mut out: Vec<String> = lines[..a].to_vec();
            out.extend(body);
            out.extend_from_slice(&lines[b..]);
            lines = out;
        }
        None => {
            lines.retain(|l| !l.trim().is_empty());
        }
    }
    join_lines(&lines, trailing)
}

/// Reverse the order of the touched lines.
fn reverse_lines(text: &str, range: Option<(usize, usize)>) -> String {
    let trailing = text.ends_with('\n');
    let mut lines = split_lines(text);
    match line_span(text, range) {
        Some((a, b)) => {
            let b = (b + 1).min(lines.len());
            lines[a..b].reverse();
        }
        None => lines.reverse(),
    }
    join_lines(&lines, trailing)
}

/// Trim leading (`lead`) and/or trailing whitespace of the touched lines.
fn trim_lines(text: &str, range: Option<(usize, usize)>, lead: bool, trail: bool) -> String {
    let trailing = text.ends_with('\n');
    let mut lines = split_lines(text);
    let (a, b) = match line_span(text, range) {
        Some((a, b)) => (a, (b + 1).min(lines.len())),
        None => (0, lines.len()),
    };
    for l in &mut lines[a..b] {
        if lead && trail {
            *l = l.trim().to_string();
        } else if trail {
            *l = l.trim_end().to_string();
        } else if lead {
            *l = l.trim_start().to_string();
        }
    }
    join_lines(&lines, trailing)
}

// ---------------------------------------------------------------- case ops

#[derive(Clone, Copy)]
pub enum CaseOp {
    Upper,
    Lower,
    Title,
    Invert,
}

/// Convert the selection (required — whole-document case changes are too
/// destructive for a menu click).
fn case_text(text: &str, range: (usize, usize), op: CaseOp) -> String {
    let (b0, b1) = byte_range(text, range);
    if b0 >= b1 {
        return text.to_string();
    }
    let sel = &text[b0..b1];
    let conv: String = match op {
        CaseOp::Upper => sel.to_uppercase(),
        CaseOp::Lower => sel.to_lowercase(),
        CaseOp::Invert => sel
            .chars()
            .map(|c| {
                if c.is_uppercase() {
                    c.to_lowercase().collect::<String>()
                } else {
                    c.to_uppercase().collect::<String>()
                }
            })
            .collect(),
        CaseOp::Title => {
            // capitalize the first alphanumeric char of every word, lowercase the rest
            let mut out = String::with_capacity(sel.len());
            let mut at_word_start = true;
            for c in sel.chars() {
                if c.is_alphanumeric() {
                    if at_word_start {
                        out.extend(c.to_uppercase());
                        at_word_start = false;
                    } else {
                        out.extend(c.to_lowercase());
                    }
                } else {
                    out.push(c);
                    at_word_start = true;
                }
            }
            out
        }
    };
    format!("{}{}{}", &text[..b0], conv, &text[b1..])
}

// ---------------------------------------------------------------- encoders

pub fn base64_encode(s: &str) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(s.as_bytes())
}

pub fn base64_decode(s: &str) -> Result<String, String> {
    use base64::Engine as _;
    let trimmed = s.trim();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(trimmed.as_bytes())
        .map_err(|e| e.to_string())?;
    String::from_utf8(bytes).map_err(|_| "invalid UTF-8 after decode".to_string())
}

/// Percent-encode everything except `A-Za-z0-9 - _ . ~`
pub fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

pub fn url_decode(s: &str) -> Result<String, String> {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hi = (bytes[i + 1] as char).to_digit(16);
                let lo = (bytes[i + 2] as char).to_digit(16);
                match (hi, lo) {
                    (Some(h), Some(l)) => {
                        out.push(((h * 16) + l) as u8);
                        i += 3;
                    }
                    _ => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| "invalid UTF-8 after decode".to_string())
}

// ---------------------------------------------------------------- hashes

pub fn md5_hex(s: &str) -> String {
    use md5::{Digest, Md5};
    let mut h = Md5::new();
    h.update(s.as_bytes());
    hex(&h.finalize())
}

pub fn sha1_hex(s: &str) -> String {
    use sha1::{Digest, Sha1};
    let mut h = Sha1::new();
    h.update(s.as_bytes());
    hex(&h.finalize())
}

pub fn sha256_hex(s: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    hex(&h.finalize())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

// ---------------------------------------------------------------- misc

/// Local timestamp `YYYY-MM-DD HH:MM:SS` (chrono `Local`).
pub fn now_stamp() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// JSON string-literal escape (keeps the surrounding quotes so the result is
/// itself a valid JSON string).
pub fn json_escape(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| s.to_string())
}

/// Inverse of `json_escape` — input must be a JSON string literal.
pub fn json_unescape(s: &str) -> Result<String, String> {
    serde_json::from_str::<String>(s.trim()).map_err(|e| format!("not a JSON string: {}", e))
}

// ---------------------------------------------------------------- dispatcher

/// Outcome of a tool: either a text replacement (new_text, caret) or an
/// informational message (hash results etc. — also copied to the clipboard).
pub enum Outcome {
    Text(String, usize),
    Info(String),
}

/// The single entry point used by `App::tool_run()`. `range`/`caret` are CHAR
/// indices into `text`. Returns Err(user-readable) on failures like a bad
/// Base64 payload or a missing selection.
pub fn apply(
    op: &str,
    text: &str,
    range: Option<(usize, usize)>,
    caret: usize,
) -> Result<Outcome, String> {
    let caret = caret.min(text.chars().count());
    match op {
        "sort_asc" => Ok(Outcome::Text(sort_lines(text, range, false), caret)),
        "sort_desc" => Ok(Outcome::Text(sort_lines(text, range, true), caret)),
        "dedupe" => Ok(Outcome::Text(dedupe_lines(text, range), caret)),
        "rm_empty" => Ok(Outcome::Text(remove_empty_lines(text, range), caret)),
        "reverse" => Ok(Outcome::Text(reverse_lines(text, range), caret)),
        "trim_trail" => Ok(Outcome::Text(trim_lines(text, range, false, true), caret)),
        "trim_lead" => Ok(Outcome::Text(trim_lines(text, range, true, false), caret)),
        "trim_both" => Ok(Outcome::Text(trim_lines(text, range, true, true), caret)),
        "upper" | "lower" | "title" | "invert" => {
            let r = range.ok_or("Select some text first")?;
            let cop = match op {
                "upper" => CaseOp::Upper,
                "lower" => CaseOp::Lower,
                "title" => CaseOp::Title,
                _ => CaseOp::Invert,
            };
            Ok(Outcome::Text(case_text(text, r, cop), r.0))
        }
        "b64_enc" => {
            let (s, at) = selection_or_word(text, range, caret);
            Ok(Outcome::Text(replace_span(text, range, at, &base64_encode(&s)), at))
        }
        "b64_dec" => {
            let (s, at) = selection_or_word(text, range, caret);
            let dec = base64_decode(&s).map_err(|e| format!("Base64 decode failed: {}", e))?;
            Ok(Outcome::Text(replace_span(text, range, at, &dec), at))
        }
        "url_enc" => {
            let (s, at) = selection_or_word(text, range, caret);
            Ok(Outcome::Text(replace_span(text, range, at, &url_encode(&s)), at))
        }
        "url_dec" => {
            let (s, at) = selection_or_word(text, range, caret);
            let dec = url_decode(&s).map_err(|e| format!("URL decode failed: {}", e))?;
            Ok(Outcome::Text(replace_span(text, range, at, &dec), at))
        }
        "json_esc" => {
            let r = range.ok_or("Select some text first")?;
            let sel = char_slice(text, r.0, r.1);
            let b0 = byte_of_char(text, r.0);
            let b1 = byte_of_char(text, r.1);
            Ok(Outcome::Text(
                format!("{}{}{}", &text[..b0], json_escape(sel), &text[b1..]),
                r.0,
            ))
        }
        "json_unesc" => {
            let r = range.ok_or("Select some text first")?;
            let sel = char_slice(text, r.0, r.1);
            let un = json_unescape(sel)?;
            let b0 = byte_of_char(text, r.0);
            let b1 = byte_of_char(text, r.1);
            Ok(Outcome::Text(
                format!("{}{}{}", &text[..b0], un, &text[b1..]),
                r.0,
            ))
        }
        "md5" | "sha1" | "sha256" => {
            let (s, _) = selection_or_word(text, range, caret);
            let hexed = match op {
                "md5" => md5_hex(&s),
                "sha1" => sha1_hex(&s),
                _ => sha256_hex(&s),
            };
            Ok(Outcome::Info(format!("{}  {}", op.to_uppercase(), hexed)))
        }
        "timestamp" => {
            let stamp = now_stamp();
            // insert at the caret (replaces an active selection if present)
            let (s, e) = range.unwrap_or((caret, caret));
            let (s, e) = (s.min(e), e.max(s));
            let head: String = text.chars().take(s).collect();
            let tail: String = text.chars().skip(e).collect();
            Ok(Outcome::Text(
                format!("{}{}{}", head, stamp, tail),
                s + stamp.chars().count(),
            ))
        }
        _ => Err(format!("unknown tool: {}", op)),
    }
}

/// Text of the selection, or — when nothing is selected — the word under the
/// caret, or the empty string. Returns (text, caret_to_use).
fn selection_or_word(text: &str, range: Option<(usize, usize)>, caret: usize) -> (String, usize) {
    match range {
        Some((s, e)) if e > s => (char_slice(text, s, e).to_string(), s),
        _ => {
            let chars: Vec<char> = text.chars().collect();
            let at = caret.min(chars.len());
            let (ws, we) = crate::complete::word_at(&chars, at);
            (chars[ws..we].iter().collect(), ws)
        }
    }
}

/// Replace `range` (or insert at `at` when the range is None/empty) with `with`.
fn replace_span(text: &str, range: Option<(usize, usize)>, at: usize, with: &str) -> String {
    match range {
        Some((s, e)) if e > s => crate::editops::replace_range(text, (s, e), with).0,
        _ => crate::editops::replace_range(text, (at, at), with).0,
    }
}

// ---------------------------------------------------------------- doc stats

/// Snapshot for the Document Statistics window.
#[derive(Clone)]
pub struct DocStats {
    pub lines: usize,
    pub words: usize,
    pub chars: usize,
    pub chars_nospace: usize,
    pub sel_chars: usize,
    pub sel_words: usize,
    pub sel_lines: usize,
    pub disk_len: u64,
    pub crlf: bool,
    pub bom: bool,
}

impl DocStats {
    pub fn compute(
        text: &str,
        sel: Option<(usize, usize)>,
        disk_len: u64,
        crlf: bool,
        bom: bool,
    ) -> Self {
        let lines = text.lines().count();
        let words = text.split_whitespace().count();
        let chars = text.chars().count();
        let chars_nospace = text.chars().filter(|c| !c.is_whitespace()).count();
        let (sel_chars, sel_words, sel_lines) = match sel.filter(|(s, e)| e > s) {
            Some((s, e)) => {
                let slice = char_slice(text, s, e);
                (
                    slice.chars().count(),
                    slice.split_whitespace().count(),
                    slice.lines().count(),
                )
            }
            None => (0, 0, 0),
        };
        Self {
            lines,
            words,
            chars,
            chars_nospace,
            sel_chars,
            sel_words,
            sel_lines,
            disk_len,
            crlf,
            bom,
        }
    }
}

// ---------------------------------------------------------------- stats window

fn human_size(n: u64) -> String {
    if n >= 1024 * 1024 {
        format!("{:.1} MB", n as f64 / (1024.0 * 1024.0))
    } else if n >= 1024 {
        format!("{:.1} KB", n as f64 / 1024.0)
    } else {
        format!("{} B", n)
    }
}

/// The "Document Statistics" window — rendered from the frame loop while
/// `app.stats_open` is true; shows the snapshot taken by `doc_stats_open()`.
pub fn show_stats_window(
    app: &mut crate::app::App,
    ctx: &egui::Context,
    t: &crate::theme::Theme,
) {
    let Some(st) = app.stats.clone() else {
        app.stats_open = false;
        return;
    };
    let name = app.active.map(|i| app.docs[i].name.clone()).unwrap_or_default();
    let path = app
        .active
        .and_then(|i| app.docs[i].path.clone())
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "(unsaved)".to_string());

    let mut open = app.stats_open;
    egui::Window::new(egui::RichText::new("Document Statistics").size(14.0))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .frame(
            egui::Frame::window(&ctx.style())
                .fill(t.bg)
                .stroke(egui::Stroke::new(1.0, t.border)),
        )
        .show(ctx, |ui| {
            egui::Grid::new("rea_doc_stats")
                .num_columns(2)
                .spacing([16.0, 3.0])
                .show(ui, |ui| {
                    let row = |ui: &mut egui::Ui, k: &str, v: String| {
                        ui.label(
                            egui::RichText::new(k)
                                .size(12.0)
                                .color(t.text_dim),
                        );
                        ui.label(egui::RichText::new(v).size(12.0).color(t.text));
                        ui.end_row();
                    };
                    row(ui, "File", name);
                    row(ui, "Path", path);
                    row(ui, "Lines", st.lines.to_string());
                    row(ui, "Words", st.words.to_string());
                    row(ui, "Characters", st.chars.to_string());
                    row(ui, "Characters (no spaces)", st.chars_nospace.to_string());
                    if st.sel_chars > 0 {
                        row(
                            ui,
                            "Selection",
                            format!(
                                "{} chars · {} words · {} lines",
                                st.sel_chars, st.sel_words, st.sel_lines
                            ),
                        );
                    }
                    row(ui, "Size on disk", human_size(st.disk_len));
                    row(
                        ui,
                        "Line endings",
                        if st.crlf { "CRLF (Windows)".to_string() } else { "LF (Unix)".to_string() },
                    );
                    row(
                        ui,
                        "Encoding",
                        if st.bom { "UTF-8 with BOM".to_string() } else { "UTF-8".to_string() },
                    );
                });
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("Selection stats appear while text is selected — reopen via Tools → Document Statistics")
                    .size(10.5)
                    .color(t.text_faint),
            );
        });
    app.stats_open = open;
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sort_whole_doc_and_range() {
        assert_eq!(sort_lines("c\nb\na\n", None, false), "a\nb\nc\n");
        assert_eq!(sort_lines("c\nb\na\n", None, true), "c\nb\na\n");
        // range 0..3 covers "c\nb" → lines 0-1 only
        assert_eq!(sort_lines("c\nb\na\n", Some((0, 3)), false), "b\nc\na\n");
    }

    #[test]
    fn sort_unicode_persian() {
        assert_eq!(sort_lines("مهر\nآبان\nدی\n", None, false), "آبان\nدی\nمهر\n");
    }

    #[test]
    fn dedupe_keeps_first_and_range_limited() {
        assert_eq!(dedupe_lines("a\nb\na\nb\nc\n", None), "a\nb\nc\n");
        // range covers only the first two lines → "a\na" collapses there
        assert_eq!(dedupe_lines("a\na\nb\n", Some((0, 3))), "a\nb\n");
    }

    #[test]
    fn remove_empty_keeps_nonempty() {
        assert_eq!(remove_empty_lines("a\n\n  \nb\n", None), "a\nb\n");
    }

    #[test]
    fn reverse_and_trim() {
        assert_eq!(reverse_lines("1\n2\n3\n", None), "3\n2\n1\n");
        assert_eq!(trim_lines("  a  \n\tb\t\n", None, false, true), "  a\n\tb\n");
        assert_eq!(trim_lines("  a  \n\tb\t\n", None, true, true), "a\nb\n");
    }

    #[test]
    fn case_ops() {
        assert_eq!(case_text("hello World", (0, 11), CaseOp::Upper), "HELLO WORLD");
        assert_eq!(case_text("hello World", (0, 11), CaseOp::Title), "Hello World");
        assert_eq!(case_text("aBc", (0, 3), CaseOp::Invert), "AbC");
        // char-safe on multibyte text
        let t = "سلام دنیا";
        assert_eq!(
            case_text(t, (0, t.chars().count()), CaseOp::Upper).chars().count(),
            t.chars().count()
        );
    }

    #[test]
    fn base64_roundtrip_and_error() {
        let src = "REA Studio — متن فارسی ۱۲۳";
        let enc = base64_encode(src);
        assert_eq!(base64_decode(&enc).unwrap(), src);
        assert!(base64_decode("!!!not-base64!!!").is_err());
    }

    #[test]
    fn url_roundtrip() {
        let src = "a b/c?d=سلام&x=1";
        let enc = url_encode(src);
        assert!(!enc.contains(' '));
        assert_eq!(url_decode(&enc).unwrap(), src);
    }

    #[test]
    fn known_hash_vectors() {
        assert_eq!(md5_hex("abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(sha1_hex("abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(
            sha256_hex("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn timestamp_shape() {
        let s = now_stamp();
        assert_eq!(s.len(), 19);
        assert_eq!(s.as_bytes()[4], b'-');
        assert_eq!(s.as_bytes()[10], b' ');
        assert_eq!(s.as_bytes()[13], b':');
    }

    #[test]
    fn json_escape_unescape_roundtrip() {
        let src = "quote \" backslash \\ newline \n";
        let esc = json_escape(src);
        assert!(esc.starts_with('"') && esc.ends_with('"'));
        assert_eq!(json_unescape(&esc).unwrap(), src);
    }

    #[test]
    fn dispatcher_text_and_info() {
        // no selection → whole doc sorted
        match apply("sort_asc", "c\nb\na\n", None, 0).unwrap() {
            Outcome::Text(t, _) => assert_eq!(t, "a\nb\nc\n"),
            _ => panic!("expected Text"),
        }
        // hash → Info
        match apply("md5", "abc", None, 0).unwrap() {
            Outcome::Info(s) => assert!(s.starts_with("MD5  9001509")),
            _ => panic!("expected Info"),
        }
        // case without selection → error message
        assert!(apply("upper", "abc", None, 0).is_err());
        // timestamp insert at caret 0
        match apply("timestamp", "abc", None, 0).unwrap() {
            Outcome::Text(t, c) => {
                assert_eq!(t.chars().count(), 19 + 3);
                assert_eq!(c, 19);
            }
            _ => panic!("expected Text"),
        }
        // url encode of an explicit selection
        match apply("url_enc", "hello world", Some((0, 11)), 0).unwrap() {
            Outcome::Text(t, _) => assert!(t.starts_with("hello%20world")),
            _ => panic!("expected Text"),
        }
    }

    #[test]
    fn doc_stats_counts() {
        let st = DocStats::compute("one two\nthree\n", Some((0, 7)), 42, false, false);
        assert_eq!(st.lines, 2);
        assert_eq!(st.words, 3);
        assert_eq!(st.chars, 14);
        assert_eq!(st.sel_chars, 7);
        assert_eq!(st.sel_lines, 1);
        assert_eq!(st.sel_words, 2);
    }
}
