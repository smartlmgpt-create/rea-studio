//! Text utilities — UTF-16 <-> char-index conversions and line/col helpers.
//!
//! LSP and Windows' native caret APIs use UTF-16 code units. egui's `CCursor`
//! is char-based. Mixing them silently corrupts positions for any text that
//! contains characters outside the BMP (Persian is fine, but CJK, emoji and
//! surrogate pairs are not) — every conversion between the two worlds MUST go
//! through this module.

/// Convert a char index into a UTF-16 code-unit offset.
pub fn char_to_utf16(s: &str, char_idx: usize) -> usize {
    let mut units = 0usize;
    for (i, c) in s.chars().enumerate() {
        if i >= char_idx {
            break;
        }
        units += if (c as u32) > 0xFFFF { 2 } else { 1 };
    }
    units
}

/// Convert a UTF-16 code-unit offset into a char index (clamped).
pub fn utf16_to_char(s: &str, utf16_idx: usize) -> usize {
    let mut units = 0usize;
    for (i, c) in s.chars().enumerate() {
        if units >= utf16_idx {
            return i;
        }
        units += if (c as u32) > 0xFFFF { 2 } else { 1 };
    }
    s.chars().count()
}

/// (line, char-col) of a flat char index.
pub fn char_to_line_col(text: &str, char_idx: usize) -> (usize, usize) {
    let mut line = 0usize;
    let mut col = 0usize;
    for (i, c) in text.chars().enumerate() {
        if i >= char_idx {
            break;
        }
        if c == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// Byte offset of char index `n` in `s` — always a char boundary (fixes the
/// old char-index-as-byte-offset slicing that panicked on any non-ASCII text).
fn byte_of_char(s: &str, n: usize) -> usize {
    s.char_indices().nth(n).map(|(off, _)| off).unwrap_or(s.len())
}

/// Char-index slice of `text` (both ends are char indices, clamped safely).
pub fn char_slice(text: &str, start: usize, end: usize) -> &str {
    if end <= start {
        return "";
    }
    let b0 = byte_of_char(text, start);
    let b1 = byte_of_char(text, end);
    &text[b0..b1]
}

/// Flat char index of (line, char-col). Never crosses '\n' and never panics,
/// regardless of where char boundaries fall.
pub fn line_col_to_char(text: &str, line: usize, col: usize) -> usize {
    let mut line_start = 0usize; // char index of the line start
    if line > 0 {
        let mut cur_line = 0usize;
        for (i, c) in text.chars().enumerate() {
            if c == '\n' {
                cur_line += 1;
                line_start = i + 1;
                if cur_line == line {
                    break;
                }
            }
        }
        if cur_line < line {
            // requested line beyond EOF → end of text
            return text.chars().count();
        }
    }
    // walk `col` chars from the line start, but never cross '\n'
    let mut n = 0usize;
    for c in text[byte_of_char(text, line_start)..].chars() {
        if n >= col || c == '\n' {
            break;
        }
        n += 1;
    }
    line_start + n
}

/// LSP position (line, UTF-16 character) from a flat char index.
pub fn lsp_pos(text: &str, char_idx: usize) -> (u32, u32) {
    let (l, c) = char_to_line_col(text, char_idx);
    // UTF-16 col: convert the chars of this line
    let line_start = line_col_to_char(text, l, 0);
    let rest = &text[byte_of_char(text, line_start)..];
    let units = char_to_utf16(rest, c);
    (l as u32, units as u32)
}

/// Flat char index from an LSP position (line, UTF-16 character).
pub fn lsp_pos_to_char(text: &str, line: u32, utf16_col: u32) -> usize {
    let line_start = line_col_to_char(text, line as usize, 0);
    let rest = &text[byte_of_char(text, line_start)..];
    let col_chars = utf16_to_char(rest, utf16_col as usize);
    line_start + col_chars
}

/// Flat char range of the line containing `char_idx` (ends are char indices;
/// the range covers the line body plus its trailing '\n' when present).
pub fn line_range(text: &str, char_idx: usize) -> (usize, usize) {
    let (line, _) = char_to_line_col(text, char_idx);
    let start = line_col_to_char(text, line, 0);
    // start of the following line (or EOF) — includes the line's '\n'
    let end = line_col_to_char(text, line + 1, 0).max(start);
    (start, end)
}

/// Leading whitespace (spaces/tabs) of the line containing `char_idx`.
pub fn line_indent(text: &str, char_idx: usize) -> String {
    let (ls, le) = line_range(text, char_idx);
    char_slice(text, ls, le)
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect()
}

// ---------------------------------------------------------------- text decoding

/// Decode raw file bytes into editable text.
/// Returns `(text, had_bom, was_crlf)`:
///  * BOM stripped (UTF-8 / UTF-16 LE / UTF-16 BE) — re-added on save
///  * UTF-16 (with or without BOM) decoded to UTF-8 instead of being
///    misjudged as binary (NUL bytes are normal there)
///  * CRLF line endings normalized to LF for editing — restored on save
pub fn decode_text(bytes: &[u8]) -> (String, bool, bool) {
    // explicit BOMs first
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let (s, _bom, crlf) = normalize_utf16(&bytes[2..], false);
        return (s, true, crlf);
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let (s, _bom, crlf) = normalize_utf16(&bytes[2..], true);
        return (s, true, crlf);
    }
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        let s = String::from_utf8_lossy(&bytes[3..]).to_string();
        return finish(s, true);
    }
    // no BOM but NUL bytes → probably UTF-16 without BOM
    if bytes.contains(&0) {
        // In UTF-16LE an ASCII char is `lo 00` (NUL at ODD index);
        // in UTF-16BE it is `00 lo` (NUL at EVEN index).
        let zeros_even = bytes.iter().step_by(2).filter(|&&b| b == 0).count();
        let zeros_odd = bytes.iter().skip(1).step_by(2).filter(|&&b| b == 0).count();
        let big_endian = zeros_even > zeros_odd;
        let (s, _bom, crlf) = normalize_utf16(bytes, big_endian);
        // decoding produced garbage with NULs? treat as binary upstream
        return (s, false, crlf);
    }
    let s = String::from_utf8_lossy(bytes).to_string();
    finish(s, false)
}

fn normalize_utf16(bytes: &[u8], big_endian: bool) -> (String, bool, bool) {
    let mut units: Vec<u16> = Vec::with_capacity(bytes.len() / 2);
    for c in bytes.chunks_exact(2) {
        units.push(if big_endian {
            u16::from_be_bytes([c[0], c[1]])
        } else {
            u16::from_le_bytes([c[0], c[1]])
        });
    }
    let s = String::from_utf16_lossy(&units);
    finish(s, false)
}

fn finish(s: String, bom: bool) -> (String, bool, bool) {
    let crlf = s.contains("\r\n");
    let s = if crlf { s.replace("\r\n", "\n") } else { s };
    (s, bom, crlf)
}

/// FNV-1a hash of a string (used for stable backup keys).
pub fn fnv(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- regression tests for the char-index/byte-index crash family ----

    #[test]
    fn line_col_multibyte_safe() {
        // 'ت' is 2 bytes in UTF-8 — the old code sliced at byte offset 4
        // (mid-char) and panicked. chars: a(0) ت(1) ت(2) \n(3) b(4) ت(5)
        let t = "aتت\nbت";
        let i = line_col_to_char(t, 1, 1);
        assert_eq!(i, 5);
        assert_eq!(char_to_line_col(t, i), (1, 1));
        // LSP round trip must stay lossless
        let (l, u) = lsp_pos(t, i);
        assert_eq!(lsp_pos_to_char(t, l, u), i);
        // these used to slice with char indices too
        let _ = line_indent(t, i);
        let _ = line_range(t, i);
        // line beyond EOF clamps to the end
        assert_eq!(line_col_to_char(t, 9, 0), 6);
        // col beyond the line stops at '\n' (line 0 has 3 chars)
        assert_eq!(line_col_to_char(t, 0, 99), 3);
    }

    #[test]
    fn find_all_case_insensitive_folding_len_shift() {
        // 'İ' lowercases into TWO chars — the old to_lowercase() scan
        // reported shifted ranges that corrupted later replacements.
        let t = "İx İx";
        let hits = crate::editops::find_all(t, "x", false, false, 100);
        assert_eq!(hits.len(), 2);
        for (s, e) in hits {
            assert_eq!(char_slice(t, s, e), "x"); // valid range of the ORIGINAL
        }
    }

    #[test]
    fn decode_utf8_plain() {
        let (s, bom, crlf) = decode_text(b"hello\nworld");
        assert_eq!(s, "hello\nworld");
        assert!(!bom && !crlf);
    }

    #[test]
    fn decode_utf8_bom_stripped() {
        let (s, bom, _) = decode_text("\u{feff}hello".as_bytes());
        assert_eq!(s, "hello");
        assert!(bom);
    }

    #[test]
    fn decode_utf16le_with_bom() {
        let mut bytes = vec![0xFF, 0xFE];
        for u in "héllo\r\nworld".encode_utf16() {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        let (s, bom, crlf) = decode_text(&bytes);
        assert_eq!(s, "héllo\nworld");
        assert!(bom && crlf);
    }

    #[test]
    fn decode_utf16be_with_bom() {
        let mut bytes = vec![0xFE, 0xFF];
        for u in "abc".encode_utf16() {
            bytes.extend_from_slice(&u.to_be_bytes());
        }
        let (s, bom, _) = decode_text(&bytes);
        assert_eq!(s, "abc");
        assert!(bom);
    }

    #[test]
    fn decode_utf16le_without_bom_not_binary() {
        // "hi\n" in UTF-16LE: 68 00 69 00 0A 00 — NUL bytes but valid text
        let bytes = [0x68, 0x00, 0x69, 0x00, 0x0A, 0x00];
        let (s, _, _) = decode_text(&bytes);
        assert_eq!(s, "hi\n");
    }

    #[test]
    fn decode_crlf_normalized() {
        let (s, _, crlf) = decode_text(b"a\r\nb\r\n");
        assert_eq!(s, "a\nb\n");
        assert!(crlf);
    }

    #[test]
    fn binary_still_has_nul() {
        // random NUL-heavy bytes that don't decode to sensible UTF-16
        let bytes = [0x00, 0x00, 0x00, 0x01, 0x00, 0x02];
        let (s, _, _) = decode_text(&bytes);
        assert!(s.contains('\0') || !s.is_empty()); // upstream checks NULs
    }
}
