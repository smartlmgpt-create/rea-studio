//! v2.0: keyboard macro — record & replay (Notepad++ counterpart).
//!
//! While recording, the app passively copies Text events and plain
//! navigation/edit keys (Ctrl-chords are NOT recorded — they are app
//! commands, not document keystrokes). Playback feeds the recorded events
//! through a tiny text engine that shares egui's char-index space, then
//! lands the result as ONE undo step.
//!
//! Note: playback is raw — live conveniences (auto-pair, auto-indent,
//! post-edit fixups) do not re-run on top of it.

/// A recorded plain key. Text keystrokes arrive as [`MEv::Text`].
#[derive(Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum MKey {
    Backspace,
    Delete,
    Enter,
    Tab,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
}

/// One recorded macro step.
#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum MEv {
    Text(String),
    Key(MKey),
}

/// Tiny text engine used by macro playback: the document as a char vec with
/// a caret + selection anchor (egui char-index space). Pure and testable.
pub struct Engine {
    pub text: Vec<char>,
    pub cur: usize,
    pub anchor: usize,
}

impl Engine {
    pub fn new(text: &str, cur: usize, anchor: usize) -> Self {
        let text: Vec<char> = text.chars().collect();
        let n = text.len();
        Self { text, cur: cur.min(n), anchor: anchor.min(n) }
    }

    fn sel(&self) -> Option<(usize, usize)> {
        if self.cur == self.anchor {
            None
        } else {
            Some((self.cur.min(self.anchor), self.cur.max(self.anchor)))
        }
    }

    fn delete_range(&mut self, s: usize, e: usize) {
        self.text.splice(s..e, std::iter::empty());
        self.cur = s;
        self.anchor = s;
    }

    fn insert(&mut self, s: &str) {
        if let Some((a, b)) = self.sel() {
            self.text.splice(a..b, std::iter::empty());
            self.cur = a;
            self.anchor = a;
        }
        let at = self.cur.min(self.text.len());
        let mut ins = s.chars();
        for (i, c) in ins.by_ref().enumerate() {
            self.text.insert(at + i, c);
        }
        self.cur = at + s.chars().count();
        self.anchor = self.cur;
    }

    pub fn event(&mut self, ev: &MEv) {
        match ev {
            MEv::Text(t) => self.insert(t),
            MEv::Key(k) => self.key(*k),
        }
    }

    fn key(&mut self, k: MKey) {
        match k {
            MKey::Backspace => {
                if let Some((a, b)) = self.sel() {
                    self.delete_range(a, b);
                } else if self.cur > 0 {
                    let c = self.cur - 1;
                    self.delete_range(c, self.cur);
                }
            }
            MKey::Delete => {
                if let Some((a, b)) = self.sel() {
                    self.delete_range(a, b);
                } else if self.cur < self.text.len() {
                    self.delete_range(self.cur, self.cur + 1);
                }
            }
            MKey::Enter => self.insert("\n"),
            MKey::Tab => self.insert("\t"),
            MKey::Left => {
                if let Some((a, _)) = self.sel() {
                    self.cur = a;
                    self.anchor = a;
                } else if self.cur > 0 {
                    self.cur -= 1;
                    self.anchor = self.cur;
                }
            }
            MKey::Right => {
                if let Some((_, b)) = self.sel() {
                    self.cur = b;
                    self.anchor = b;
                } else if self.cur < self.text.len() {
                    self.cur += 1;
                    self.anchor = self.cur;
                }
            }
            MKey::Home => {
                let s = line_start(&self.text, self.cur);
                self.cur = s;
                self.anchor = s;
            }
            MKey::End => {
                let e = line_end(&self.text, self.cur);
                self.cur = e;
                self.anchor = e;
            }
            MKey::Up => self.line_move(-1),
            MKey::Down => self.line_move(1),
        }
    }

    fn line_move(&mut self, dl: isize) {
        let (line, col) = char_to_line_col(&self.text, self.cur);
        let n_lines = 1 + self.text.iter().filter(|&&c| c == '\n').count();
        let target = ((line as isize + dl).max(0) as usize).min(n_lines - 1);
        self.cur = line_col_to_char_idx(&self.text, target, col);
        self.anchor = self.cur;
    }

    pub fn text(&self) -> String {
        self.text.iter().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

// ---------------------------------------------------------------- helpers

fn char_to_line_col(chars: &[char], ch: usize) -> (usize, usize) {
    let mut line = 0usize;
    let mut col = 0usize;
    for (i, &c) in chars.iter().enumerate() {
        if i >= ch {
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

fn line_col_to_char_idx(chars: &[char], line: usize, col: usize) -> usize {
    let mut seen_line = 0usize;
    let mut idx = 0usize;
    let mut n = 0usize;
    while idx < chars.len() {
        if seen_line == line {
            break;
        }
        if chars[idx] == '\n' {
            seen_line += 1;
        }
        idx += 1;
    }
    while idx < chars.len() && chars[idx] != '\n' && n < col {
        idx += 1;
        n += 1;
    }
    idx
}

fn line_start(chars: &[char], cur: usize) -> usize {
    let mut i = cur.min(chars.len());
    while i > 0 && chars[i - 1] != '\n' {
        i -= 1;
    }
    i
}

fn line_end(chars: &[char], cur: usize) -> usize {
    let mut i = cur.min(chars.len());
    while i < chars.len() && chars[i] != '\n' {
        i += 1;
    }
    i
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;

    fn play(text: &str, evs: &[MEv]) -> String {
        let mut e = Engine::new(text, text.chars().count(), text.chars().count());
        for ev in evs {
            e.event(ev);
        }
        e.text()
    }

    #[test]
    fn type_multiline() {
        let out = play("", &[t("ab"), k(MKey::Enter), t("cd")]);
        assert_eq!(out, "ab\ncd");
    }

    #[test]
    fn backspace_and_delete() {
        let out = play("ab", &[k(MKey::Backspace)]);
        assert_eq!(out, "a");
        let mut e = Engine::new("abc", 1, 1);
        e.event(&k(MKey::Delete));
        assert_eq!(e.text(), "ac");
    }

    #[test]
    fn selection_replaced_by_typing() {
        // select "b" (index 1..2) and type "X"
        let mut e = Engine::new("abc", 2, 1);
        e.event(&t("X"));
        assert_eq!(e.text(), "aXc");
        assert_eq!(e.cur, 2);
    }

    #[test]
    fn arrows_and_home_end() {
        // at end of "hello\nworld": Home → col 0, Right×3 → col 3, End → col 5
        let mut e = Engine::new("hello\nworld", 11, 11);
        e.event(&k(MKey::Home));
        assert_eq!(e.cur, 6);
        e.event(&k(MKey::Right));
        e.event(&k(MKey::Right));
        e.event(&k(MKey::Right));
        assert_eq!(e.cur, 9);
        e.event(&k(MKey::End));
        assert_eq!(e.cur, 11);
    }

    #[test]
    fn up_down_keeps_column() {
        // "abcd\nxyz" — from (1, 2) go Up → (0, 2)
        let mut e = Engine::new("abcd\nxyz", 7, 7);
        e.event(&k(MKey::Up));
        assert_eq!(e.cur, 2);
        e.event(&k(MKey::Down));
        assert_eq!(e.cur, 7); // col 2 clamped to line 2 len (3)
    }

    #[test]
    fn replay_returns_same_text_on_same_state() {
        // recording "hi" played on empty doc twice appends twice
        let evs = vec![t("hi")];
        assert_eq!(play("", &evs), "hi");
        assert_eq!(play("hi", &evs), "hihi");
    }

    fn t(s: &str) -> MEv {
        MEv::Text(s.to_string())
    }
    fn k(k: MKey) -> MEv {
        MEv::Key(k)
    }
}
