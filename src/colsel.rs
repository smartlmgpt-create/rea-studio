//! v2.0: column (block) selection — the Notepad++ "Column Mode" counterpart.
//!
//! State: two corners (anchor, current) in (line, char-col) space. Typing
//! replaces the block with the typed text on EVERY line in range; Backspace
//! deletes the block (or one char before it when the block is zero-width);
//! Delete deletes the block (or one char at it). Esc exits.
//!
//! All operations are pure string-in/string-out functions (testable); the App
//! wrappers own undo/LSP bookkeeping.

/// Column-selection state (two corners, line/char-col each).
#[derive(Clone, Copy, PartialEq)]
pub struct ColSel {
    pub doc: usize,
    /// anchor corner (line, col)
    pub anchor: (usize, usize),
    /// moving corner (line, col)
    pub cur: (usize, usize),
}

impl ColSel {
    pub fn lines(&self) -> (usize, usize) {
        (self.anchor.0.min(self.cur.0), self.anchor.0.max(self.cur.0))
    }
    pub fn cols(&self) -> (usize, usize) {
        (self.anchor.1.min(self.cur.1), self.anchor.1.max(self.cur.1))
    }
}

/// Insert `s` at column `col` into every line in `la..=lb` (inclusive).
/// Lines shorter than `col` insert at their end (no padding). Returns the
/// new text and the caret column after the insert.
pub fn insert_at_col(text: &str, la: usize, lb: usize, col: usize, s: &str) -> (String, usize) {
    let lines: Vec<String> = text.split('\n').map(|l| l.to_string()).collect();
    let n = lines.len();
    let la = la.min(n - 1);
    let lb = lb.min(n - 1);
    let ins: Vec<char> = s.chars().collect();
    let mut out: Vec<String> = Vec::with_capacity(n);
    for (i, line) in lines.iter().enumerate() {
        if i >= la && i <= lb && !ins.is_empty() {
            let chars: Vec<char> = line.chars().collect();
            let at = col.min(chars.len());
            let mut nl: String = chars[..at].iter().collect();
            nl.extend(&ins);
            nl.extend(chars[at..].iter());
            out.push(nl);
        } else {
            out.push(line.clone());
        }
    }
    (out.join("\n"), col + s.chars().count())
}

/// Delete columns `[cs, ce)` from every line in `la..=lb` (clamped per line).
/// Returns the new text and the caret column after the delete (`cs`).
pub fn delete_at_col(text: &str, la: usize, lb: usize, cs: usize, ce: usize) -> (String, usize) {
    let ce = ce.max(cs);
    if ce == cs {
        return (text.to_string(), cs);
    }
    let lines: Vec<String> = text.split('\n').map(|l| l.to_string()).collect();
    let n = lines.len();
    let la = la.min(n - 1);
    let lb = lb.min(n - 1);
    let mut out: Vec<String> = Vec::with_capacity(n);
    for (i, line) in lines.iter().enumerate() {
        if i >= la && i <= lb {
            let chars: Vec<char> = line.chars().collect();
            let s = cs.min(chars.len());
            let e = ce.min(chars.len()).max(s);
            let nl: String = chars[..s].iter().chain(chars[e..].iter()).collect();
            out.push(nl);
        } else {
            out.push(line.clone());
        }
    }
    (out.join("\n"), cs)
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_three_lines() {
        let t = "foo\nbar\nbaz";
        // lines 0..=1 only — "baz" (line 2) must stay untouched
        let (out, col) = insert_at_col(t, 0, 1, 1, "#");
        assert_eq!(out, "f#oo\nb#ar\nbaz");
        assert_eq!(col, 2);
    }

    #[test]
    fn insert_short_line_clamps() {
        let t = "abc\ndef\nx";
        let (out, _) = insert_at_col(t, 0, 2, 3, "-");
        assert_eq!(out, "abc-\ndef-\nx-");
    }

    #[test]
    fn insert_on_empty_and_last_lines() {
        let t = "\n\nend";
        let (out, _) = insert_at_col(t, 0, 2, 0, ">>");
        assert_eq!(out, ">>\n>>\n>>end");
    }

    #[test]
    fn delete_block() {
        let t = "abc\nxyz\nkeep";
        let (out, col) = delete_at_col(t, 0, 1, 1, 2);
        assert_eq!(out, "ac\nxz\nkeep");
        assert_eq!(col, 1);
    }

    #[test]
    fn delete_clamped_on_short_lines() {
        let t = "ab\nabcdef\nzz";
        // [2..5) — "ab"/"zz" too short (clamped to no-op), "abcdef" loses c,d,e
        let (out, _) = delete_at_col(t, 0, 2, 2, 5);
        assert_eq!(out, "ab\nabf\nzz");
    }

    #[test]
    fn zero_width_delete_is_noop() {
        let t = "abc";
        let (out, col) = delete_at_col(t, 0, 0, 1, 1);
        assert_eq!(out, "abc");
        assert_eq!(col, 1);
    }

    #[test]
    fn type_replaces_block() {
        // block [1..3) on lines 0..=1, typed "Z" → the block is replaced
        // ("abc" clamps to [1..3) = "bc" → "a"; "xyzw" loses y,z → "xw")
        let t = "abc\nxyzw\nqqq";
        let (t1, _) = delete_at_col(t, 0, 1, 1, 3);
        assert_eq!(t1, "a\nxw\nqqq");
        let (out, col) = insert_at_col(&t1, 0, 1, 1, "Z");
        assert_eq!(out, "aZ\nxZw\nqqq");
        assert_eq!(col, 2);
    }
}
