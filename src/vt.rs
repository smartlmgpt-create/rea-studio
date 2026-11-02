//! v1.7 — minimal VT/ANSI terminal emulator (grid of styled cells).
//!
//! Feeds the raw bytes a ConPTY child emits and maintains a fixed grid of
//! cells (char + colors + attributes) that the terminal panel renders with
//! egui. Supports the escape repertoire real console programs use:
//!   * CUP/CUU/CUD/CUF/CUB/CHA/VPA cursor addressing
//!   * ED/EL/ECH erase, ICH/DCH insert/delete chars, IL/DL lines
//!   * SGR colors (16 + 256 + truecolor), bold/italic/underline
//!   * DECSTBM scroll regions + SU/SD scrolling, DECSC/DECRC save/restore
//!   * Alternate screen (1049/1047/47) so vim/less/htop work
//!   * DSR/DA replies (cursor position / device attributes) via a reply queue
//! Scrollback is kept for the primary buffer (mouse wheel scrolls it).

use eframe::egui::Color32;

// ---------------------------------------------------------------- palette

/// VS Code-ish ANSI-16 palette — readable on dark AND light themes.
pub fn ansi_color(n: u8) -> Color32 {
    match n {
        0 => Color32::from_rgb(102, 102, 102),
        1 => Color32::from_rgb(0xE0, 0x55, 0x61),
        2 => Color32::from_rgb(0x4E, 0xC9, 0x4E),
        3 => Color32::from_rgb(0xD7, 0xBA, 0x4F),
        4 => Color32::from_rgb(0x56, 0x9C, 0xD6),
        5 => Color32::from_rgb(0xC5, 0x86, 0xC0),
        6 => Color32::from_rgb(0x4E, 0xC9, 0xB0),
        7 => Color32::from_rgb(0xD4, 0xD4, 0xD4),
        8 => Color32::from_rgb(0x8A, 0x8A, 0x8A),
        9 => Color32::from_rgb(0xF1, 0x4C, 0x4C),
        10 => Color32::from_rgb(0x67, 0xE2, 0x67),
        11 => Color32::from_rgb(0xE8, 0xD1, 0x6B),
        12 => Color32::from_rgb(0x6F, 0xB3, 0xEF),
        13 => Color32::from_rgb(0xD8, 0x9B, 0xD5),
        14 => Color32::from_rgb(0x6B, 0xD9, 0xC8),
        _ => Color32::from_rgb(0xF2, 0xF2, 0xF2),
    }
}

// ---------------------------------------------------------------- cell

#[derive(Clone, PartialEq)]
pub struct Cell {
    pub ch: char,
    pub fg: Color32,
    pub bg: Color32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

impl Cell {
    fn blank(fg: Color32, bg: Color32) -> Self {
        Cell { ch: ' ', fg, bg, bold: false, italic: false, underline: false }
    }
    pub fn is_blank(&self) -> bool {
        self.ch == ' ' && !self.bold && !self.italic && !self.underline
    }
}

// ---------------------------------------------------------------- emulator

#[derive(Clone, Copy, PartialEq)]
enum St {
    Ground,
    Esc,
    Csi,
    Osc, // OSC / DCS / PM / APC — swallow until BEL or ST
}

pub struct Vt {
    pub cols: usize,
    pub rows: usize,
    grid: Vec<Cell>,
    alt: Option<Vec<Cell>>, // alternate screen buffer
    pub alt_active: bool,

    cur: (usize, usize), // row, col
    saved: ((usize, usize), Cell),
    fg: Color32,
    bg: Color32,
    bold: bool,
    italic: bool,
    underline: bool,
    cursor_visible: bool,

    top: usize,    // scroll region top (inclusive)
    bot: usize,    // scroll region bottom (inclusive)
    wrap_pending: bool,

    st: St,
    csi_priv: bool, // CSI ? prefix
    params: String,
    /// charset-designation follower flag (ESC ( B)
    charset_next: bool,

    pub title: String,
    /// replies the emulator wants written back to the child (DSR/DA)
    pub replies: Vec<String>,
    pub scrollback: Vec<Vec<Cell>>,
}

impl Vt {
    pub fn new(cols: usize, rows: usize, default_fg: Color32, default_bg: Color32) -> Self {
        let mut v = Vt {
            cols: cols.max(20),
            rows: rows.max(6),
            grid: Vec::new(),
            alt: None,
            alt_active: false,
            cur: (0, 0),
            saved: ((0, 0), Cell::blank(default_fg, default_bg)),
            fg: default_fg,
            bg: default_bg,
            bold: false,
            italic: false,
            underline: false,
            cursor_visible: true,
            top: 0,
            bot: 0,
            wrap_pending: false,
            st: St::Ground,
            csi_priv: false,
            params: String::new(),
            charset_next: false,
            title: String::new(),
            replies: Vec::new(),
            scrollback: Vec::new(),
        };
        v.bot = v.rows - 1;
        v.grid = vec![Cell::blank(default_fg, default_bg); v.cols * v.rows];
        v
    }

    fn blank(&self) -> Cell {
        Cell::blank(self.fg, self.bg)
    }

    pub fn cursor_visible(&self) -> bool {
        self.cursor_visible
    }
    pub fn cursor(&self) -> (usize, usize) {
        self.cur
    }
    pub fn cell(&self, r: usize, c: usize) -> &Cell {
        &self.grid[r * self.cols + c]
    }
    pub fn row(&self, r: usize) -> &[Cell] {
        &self.grid[r * self.cols..(r + 1) * self.cols]
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        let cols = cols.max(20).min(500);
        let rows = rows.max(6).min(300);
        if cols == self.cols && rows == self.rows {
            return;
        }
        let b = self.blank();
        let mut ng = vec![b.clone(); cols * rows];
        for r in 0..rows.min(self.rows) {
            for c in 0..cols.min(self.cols) {
                ng[r * cols + c] = self.grid[r * self.cols + c].clone();
            }
        }
        self.grid = ng;
        if let Some(a) = &mut self.alt {
            let mut na = vec![b.clone(); cols * rows];
            for r in 0..rows.min(self.rows) {
                for c in 0..cols.min(self.cols) {
                    if r * self.cols + c < a.len() {
                        na[r * cols + c] = a[r * self.cols + c].clone();
                    }
                }
            }
            *a = na;
        }
        self.cols = cols;
        self.rows = rows;
        self.top = 0;
        self.bot = rows - 1;
        self.cur.0 = self.cur.0.min(rows - 1);
        self.cur.1 = self.cur.1.min(cols - 1);
    }

    pub fn clear(&mut self) {
        let b = self.blank();
        for c in self.grid.iter_mut() {
            *c = b.clone();
        }
        // FIX: the trash button ("clear") used to leave 600 lines of
        // scrollback visible above the cleared screen — clear it too.
        self.scrollback.clear();
        self.cur = (0, 0);
        self.wrap_pending = false;
    }

    // ------------------------------------------------------------ scroll ops

    fn scroll_up(&mut self, n: usize) {
        let (t, b) = (self.top, self.bot);
        for _ in 0..n.max(1) {
            let row: Vec<Cell> = self.grid[t * self.cols..(t + 1) * self.cols].to_vec();
            if !self.alt_active {
                self.scrollback.push(row);
                if self.scrollback.len() > 600 {
                    self.scrollback.remove(0);
                }
            }
            for r in t..b {
                for c in 0..self.cols {
                    self.grid[r * self.cols + c] = self.grid[(r + 1) * self.cols + c].clone();
                }
            }
            for c in 0..self.cols {
                self.grid[b * self.cols + c] = self.blank();
            }
        }
    }

    fn scroll_down(&mut self, n: usize) {
        let (t, b) = (self.top, self.bot);
        for _ in 0..n.max(1) {
            for r in (t..b).rev() {
                for c in 0..self.cols {
                    self.grid[(r + 1) * self.cols + c] = self.grid[r * self.cols + c].clone();
                }
            }
            for c in 0..self.cols {
                self.grid[t * self.cols + c] = self.blank();
            }
        }
    }

    fn newline(&mut self) {
        self.wrap_pending = false;
        if self.cur.0 == self.bot {
            self.scroll_up(1);
        } else if self.cur.0 + 1 < self.rows {
            self.cur.0 += 1;
        }
    }

    fn putc(&mut self, ch: char) {
        if self.wrap_pending {
            self.cur.1 = 0;
            self.newline();
        }
        let cell = Cell {
            ch,
            fg: self.fg,
            bg: self.bg,
            bold: self.bold,
            italic: self.italic,
            underline: self.underline,
        };
        let idx = self.cur.0 * self.cols + self.cur.1;
        if let Some(c) = self.grid.get_mut(idx) {
            *c = cell;
        }
        if self.cur.1 + 1 >= self.cols {
            self.cur.1 = self.cols - 1; // stay — wrap happens on next char
            self.wrap_pending = true;
        } else {
            self.cur.1 += 1;
        }
    }

    fn erase_row(&mut self, r: usize, from: usize, to: usize) {
        let b = self.blank();
        for c in from..to.min(self.cols) {
            let idx = r * self.cols + c;
            if let Some(cell) = self.grid.get_mut(idx) {
                *cell = b.clone();
            }
        }
    }

    // ------------------------------------------------------------ SGR

    fn sgr(&mut self, codes: &[u32]) {
        if codes.is_empty() {
            self.fg = self.fg; // default reset handled below
            self.reset_attrs();
            return;
        }
        let mut i = 0;
        while i < codes.len() {
            match codes[i] {
                0 => self.reset_attrs(),
                1 => self.bold = true,
                2 => {} // faint
                3 => self.italic = true,
                4 => self.underline = true,
                7 => {} // reverse — approximate: swap for the run below
                21 | 22 => self.bold = false,
                23 => self.italic = false,
                24 => self.underline = false,
                27 => {}
                30..=37 => self.fg = ansi_color((codes[i] - 30) as u8),
                39 => self.fg = ansi_color(7),
                40..=47 => self.bg = ansi_color((codes[i] - 40) as u8),
                49 => self.bg = Color32::TRANSPARENT,
                90..=97 => self.fg = ansi_color((codes[i] - 90 + 8) as u8),
                100..=107 => self.bg = ansi_color((codes[i] - 100 + 8) as u8),
                38 | 48 => {
                    // extended color: 38;5;n or 38;2;r;g;b
                    let is_fg = codes[i] == 38;
                    let col = if codes.get(i + 1) == Some(&5) {
                        let n = codes.get(i + 2).copied().unwrap_or(0) as u8;
                        i += 2;
                        xterm256(n)
                    } else if codes.get(i + 1) == Some(&2) {
                        let r = codes.get(i + 2).copied().unwrap_or(0) as u8;
                        let g = codes.get(i + 3).copied().unwrap_or(0) as u8;
                        let b = codes.get(i + 4).copied().unwrap_or(0) as u8;
                        i += 4;
                        Color32::from_rgb(r, g, b)
                    } else {
                        i += 1;
                        continue;
                    };
                    if is_fg {
                        self.fg = col;
                    } else {
                        self.bg = col;
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }

    fn reset_attrs(&mut self) {
        self.fg = ansi_color(7);
        self.bg = Color32::TRANSPARENT;
        self.bold = false;
        self.italic = false;
        self.underline = false;
    }

    // ------------------------------------------------------------ feed

    pub fn feed(&mut self, data: &str) {
        for ch in data.chars() {
            match self.st {
                St::Ground => self.ground(ch),
                St::Esc => self.esc(ch),
                St::Csi => self.csi(ch),
                St::Osc => {
                    // swallow OSC/DCS until terminator
                    if ch == '\u{7}' || (ch == '\\') {
                        self.st = St::Ground;
                    }
                }
            }
        }
    }

    fn ground(&mut self, ch: char) {
        match ch {
            '\u{1b}' => self.st = St::Esc,
            '\r' => {
                self.cur.1 = 0;
                self.wrap_pending = false;
            }
            '\n' | '\u{b}' | '\u{c}' => self.newline(),
            '\u{8}' => {
                self.wrap_pending = false;
                self.cur.1 = self.cur.1.saturating_sub(1);
            }
            '\t' => {
                let nc = ((self.cur.1 / 8) + 1) * 8;
                self.cur.1 = nc.min(self.cols - 1);
                self.wrap_pending = false;
            }
            '\u{7}' => {}
            '\u{e}' | '\u{f}' => {}
            c if (c as u32) < 32 => {}
            c => self.putc(c),
        }
    }

    fn esc(&mut self, ch: char) {
        self.st = St::Ground;
        match ch {
            '[' => {
                self.st = St::Csi;
                self.csi_priv = false;
                self.params.clear();
            }
            ']' | 'P' | '^' | '_' => self.st = St::Osc,
            '7' => self.saved = (self.cur, self.cur_cell()),
            '8' => {
                let ((r, c), cell) = self.saved.clone();
                self.cur = (r.min(self.rows - 1), c.min(self.cols - 1));
                self.fg = cell.fg;
                self.bg = cell.bg;
                self.bold = cell.bold;
                self.italic = cell.italic;
                self.underline = cell.underline;
                self.wrap_pending = false;
            }
            'D' => self.newline(),
            'M' => {
                // reverse index
                if self.cur.0 == self.top {
                    self.scroll_down(1);
                } else {
                    self.cur.0 = self.cur.0.saturating_sub(1);
                }
            }
            'E' => {
                self.cur.1 = 0;
                self.newline();
            }
            'c' => {
                // full reset
                self.top = 0;
                self.bot = self.rows - 1;
                self.cur = (0, 0);
                self.reset_attrs();
                self.cursor_visible = true;
            }
            '(' | ')' | '*' | '+' => {
                // charset designation — the NEXT char is swallowed by csi_dispatch
                self.charset_next = true;
            }
            '=' | '>' | '\\' | 'H' => {}
            _ => {}
        }
    }

    fn cur_cell(&self) -> Cell {
        let idx = self.cur.0 * self.cols + self.cur.1;
        self.grid
            .get(idx)
            .cloned()
            .unwrap_or_else(|| self.blank())
    }

    fn csi(&mut self, ch: char) {
        match ch {
            '?' | '>' | '<' | '=' if self.params.is_empty() => self.csi_priv = true,
            '0'..='9' | ';' | ':' => self.params.push(ch),
            'm' if !self.csi_priv => {
                let codes: Vec<u32> = self
                    .params
                    .split(';')
                    .map(|p| p.parse::<u32>().unwrap_or(0))
                    .collect();
                self.sgr(&codes);
                self.st = St::Ground;
            }
            _ => {
                self.csi_dispatch(ch);
                self.st = St::Ground;
            }
        }
    }

    fn priv_mode(&mut self, set: bool, n: u32) {
        match n {
            25 => self.cursor_visible = set,
            1049 => {
                if set {
                    self.saved = (self.cur, self.cur_cell());
                    let b = self.blank();
                    self.alt = Some(vec![b; self.cols * self.rows]);
                    self.alt_active = true;
                    self.cur = (0, 0);
                } else {
                    self.alt = None;
                    self.alt_active = false;
                    let ((r, c), cell) = self.saved.clone();
                    self.cur = (r.min(self.rows - 1), c.min(self.cols - 1));
                    self.fg = cell.fg;
                    self.bg = cell.bg;
                }
            }
            1047 | 47 => {
                if set {
                    if self.alt.is_none() {
                        let b = self.blank();
                        self.alt = Some(vec![b; self.cols * self.rows]);
                    }
                    self.alt_active = true;
                    self.cur = (0, 0);
                } else {
                    self.alt_active = false;
                    self.cur = (0, 0);
                }
            }
            2004 => {} // bracketed paste — accepted, ignored
            _ => {}
        }
    }

    fn n_params(&self) -> Vec<usize> {
        self.params
            .split(';')
            .map(|p| p.parse::<usize>().unwrap_or(0))
            .collect()
    }

    fn one(&self, dflt: usize) -> usize {
        let p = self.params.split(';').next().unwrap_or("");
        let v: usize = p.parse().unwrap_or(0);
        if v == 0 {
            dflt
        } else {
            v
        }
    }

    fn csi_dispatch(&mut self, fin: char) {
        // charset follower consumed here (ESC ( B style)
        if self.charset_next {
            self.charset_next = false;
            return;
        }
        // private modes h/l
        if self.csi_priv && matches!(fin, 'h' | 'l') {
            let ns = self.n_params();
            for n in ns {
                self.priv_mode(fin == 'h', n as u32);
            }
            self.csi_priv = false;
            return;
        }
        match fin {
            'A' => {
                self.cur.0 = self.cur.0.saturating_sub(self.one(1));
                self.wrap_pending = false;
            }
            'B' | 'e' => {
                self.cur.0 = (self.cur.0 + self.one(1)).min(self.rows - 1);
                self.wrap_pending = false;
            }
            'C' | 'a' => {
                self.cur.1 = (self.cur.1 + self.one(1)).min(self.cols - 1);
                self.wrap_pending = false;
            }
            'D' => {
                self.cur.1 = self.cur.1.saturating_sub(self.one(1));
                self.wrap_pending = false;
            }
            'E' => {
                self.cur.0 = (self.cur.0 + self.one(1)).min(self.rows - 1);
                self.cur.1 = 0;
            }
            'F' => {
                self.cur.0 = self.cur.0.saturating_sub(self.one(1));
                self.cur.1 = 0;
            }
            'G' | '`' => {
                self.cur.1 = (self.one(1).saturating_sub(1)).min(self.cols - 1);
                self.wrap_pending = false;
            }
            'd' => {
                self.cur.0 = (self.one(1).saturating_sub(1)).min(self.rows - 1);
                self.wrap_pending = false;
            }
            'H' | 'f' => {
                let p = self.n_params();
                let r = p.first().copied().unwrap_or(1).max(1);
                let c = p.get(1).copied().unwrap_or(1).max(1);
                self.cur.0 = (r - 1).min(self.rows - 1);
                self.cur.1 = (c - 1).min(self.cols - 1);
                self.wrap_pending = false;
            }
            'J' => {
                let m = self.params.split(';').next().unwrap_or("").parse::<u32>().unwrap_or(0);
                let (cr, cc) = self.cur;
                match m {
                    0 => {
                        self.erase_row(cr, cc, self.cols);
                        for r in cr + 1..self.rows {
                            self.erase_row(r, 0, self.cols);
                        }
                    }
                    1 => {
                        self.erase_row(cr, 0, cc + 1);
                        for r in 0..cr {
                            self.erase_row(r, 0, self.cols);
                        }
                    }
                    _ => {
                        for r in 0..self.rows {
                            self.erase_row(r, 0, self.cols);
                        }
                    }
                }
            }
            'K' => {
                let m = self.params.split(';').next().unwrap_or("").parse::<u32>().unwrap_or(0);
                let (cr, cc) = self.cur;
                match m {
                    0 => self.erase_row(cr, cc, self.cols),
                    1 => self.erase_row(cr, 0, cc + 1),
                    _ => self.erase_row(cr, 0, self.cols),
                }
            }
            'X' => {
                let n = self.one(1);
                self.erase_row(self.cur.0, self.cur.1, self.cur.1 + n);
            }
            'P' => {
                // delete n chars at cursor
                let n = self.one(1);
                let r = self.cur.0;
                let c0 = self.cur.1;
                for c in c0..self.cols {
                    let src = c + n;
                    let v = if src < self.cols {
                        self.grid[r * self.cols + src].clone()
                    } else {
                        self.blank()
                    };
                    self.grid[r * self.cols + c] = v;
                }
            }
            '@' => {
                // insert n blank chars at cursor
                let n = self.one(1);
                let r = self.cur.0;
                let c0 = self.cur.1;
                let mut c = self.cols.saturating_sub(1);
                while c > c0 {
                    let src = c.checked_sub(n).unwrap_or(0);
                    self.grid[r * self.cols + c] =
                        if src >= c0 { self.grid[r * self.cols + src].clone() } else { self.blank() };
                    c -= 1;
                }
            }
            'L' => {
                // insert n lines at cursor (within margins)
                let n = self.one(1);
                if self.cur.0 >= self.top && self.cur.0 <= self.bot {
                    let r = self.cur.0;
                    let save_bot = self.bot;
                    self.bot = save_bot;
                    let _ = self.top;
                    // shift lines down
                    let mut rr = save_bot;
                    while rr > r {
                        let src = rr.checked_sub(n).unwrap_or(0);
                        for c in 0..self.cols {
                            self.grid[rr * self.cols + c] = if src >= r {
                                self.grid[src * self.cols + c].clone()
                            } else {
                                self.blank()
                            };
                        }
                        if rr == 0 {
                            break;
                        }
                        rr -= 1;
                    }
                    for c in 0..self.cols {
                        self.grid[r * self.cols + c] = self.blank();
                    }
                }
            }
            'M' => {
                // delete n lines at cursor
                let n = self.one(1);
                if self.cur.0 >= self.top && self.cur.0 <= self.bot {
                    let r = self.cur.0;
                    for rr in r..=self.bot {
                        let src = rr + n;
                        for c in 0..self.cols {
                            self.grid[rr * self.cols + c] = if src <= self.bot {
                                self.grid[src * self.cols + c].clone()
                            } else {
                                self.blank()
                            };
                        }
                    }
                }
            }
            'S' => self.scroll_up(self.one(1)),
            'T' => self.scroll_down(self.one(1)),
            'r' => {
                let p = self.n_params();
                let t = p.first().copied().unwrap_or(1).max(1);
                let b = p.get(1).copied().unwrap_or(self.rows).max(1);
                self.top = (t - 1).min(self.rows - 1);
                self.bot = (b - 1).min(self.rows - 1);
                if self.top > self.bot {
                    self.top = 0;
                    self.bot = self.rows - 1;
                }
                self.cur = (self.top, 0);
            }
            's' => self.saved = (self.cur, self.cur_cell()),
            'u' => {
                let ((r, c), cell) = self.saved.clone();
                self.cur = (r.min(self.rows - 1), c.min(self.cols - 1));
                self.fg = cell.fg;
                self.bg = cell.bg;
                self.bold = cell.bold;
                self.italic = cell.italic;
                self.underline = cell.underline;
            }
            'n' => {
                // DSR
                if self.one(1) == 6 {
                    self.replies
                        .push(format!("\u{1b}[{};{}R", self.cur.0 + 1, self.cur.1 + 1));
                }
            }
            'c' => {
                // DA — reply VT102
                self.replies.push("\u{1b}[?6c".to_string());
            }
            'h' | 'l' => {
                let ns = self.n_params();
                for n in ns {
                    self.priv_mode(fin == 'h', n as u32);
                }
            }
            _ => {}
        }
    }
}

/// xterm 256-color palette (0..=255)
fn xterm256(n: u8) -> Color32 {
    match n {
        0..=15 => ansi_color(n),
        232..=255 => {
            let v = 8 + (n - 232) as u8 * 10;
            Color32::from_rgb(v, v, v)
        }
        _ => {
            let i = (n - 16) as usize;
            let r = i / 36;
            let g = (i % 36) / 6;
            let b = i % 6;
            let ch = |v: usize| if v == 0 { 0 } else { 55 + v as u8 * 40 };
            Color32::from_rgb(ch(r), ch(g), ch(b))
        }
    }
}
