//! OUTLINE — the Function List panel (v1.9, the Notepad++ counterpart).
//!
//! Scans the active document for definitions (functions, classes, headings)
//! with one compiled regex set per language family. Runs on the UI thread but
//! only when the document changed (edit-epoch signature), so a 10k-line file
//! is re-scanned per edit at most once and typical files take microseconds.
//!
//! Kinds are drawn with small glyphs in the panel: fn ƒ, class/struct ⬡,
//! heading §.

use std::sync::OnceLock;

use regex::Regex;

#[derive(Clone, Debug)]
pub struct Sym {
    /// 0-based line number
    pub line: usize,
    pub name: String,
    pub kind: Kind,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Fn,
    Class,
    Heading,
}

impl Kind {
    pub fn glyph(self) -> &'static str {
        match self {
            Kind::Fn => "\u{192}",
            Kind::Class => "\u{2b21}",
            Kind::Heading => "\u{a7}",
        }
    }
}

pub fn scan(ext: &str, text: &str) -> Vec<Sym> {
    match ext {
        "rs" => scan_rust(text),
        "py" | "pyw" => scan_py(text),
        "go" => scan_go(text),
        "lua" => scan_lua(text),
        "rb" => scan_rb(text),
        "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx" | "php" => scan_script(text),
        "md" | "markdown" => scan_md(text),
        // C, C++, C#, Java, Kotlin, Swift, Dart, Vala, ... — one best-effort
        "c" | "h" | "cpp" | "cc" | "cxx" | "hpp" | "hh" | "cs" | "java" | "kt" | "kts"
        | "swift" | "dart" | "vala" | "groovy" | "scala" => scan_clike(text),
        _ => Vec::new(),
    }
}

fn re_cell<'a>(cell: &'a OnceLock<Regex>, pat: &str) -> &'a Regex {
    cell.get_or_init(|| Regex::new(pat).expect("outline regex"))
}

// ------------------------------------------------------------------ Rust

fn scan_rust(text: &str) -> Vec<Sym> {
    static FN: OnceLock<Regex> = OnceLock::new();
    static TY: OnceLock<Regex> = OnceLock::new();
    let fn_re = re_cell(&FN, r#"^\s*(pub(\([^)]*\))?\s+)?(const\s+)?(async\s+)?(unsafe\s+)?(extern\s+("[^"]*")?\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)"#);
    let ty_re = re_cell(&TY, r"^\s*(pub(\([^)]*\))?\s+)?(struct|enum|trait|mod|union)\s+([A-Za-z_][A-Za-z0-9_]*)");
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if let Some(c) = fn_re.captures(line) {
            out.push(Sym { line: i, name: c[c.len() - 1].to_string(), kind: Kind::Fn });
        } else if let Some(c) = ty_re.captures(line) {
            out.push(Sym { line: i, name: c[c.len() - 1].to_string(), kind: Kind::Class });
        }
    }
    out
}

// ------------------------------------------------------------------ Python

fn scan_py(text: &str) -> Vec<Sym> {
    static DEF: OnceLock<Regex> = OnceLock::new();
    static CLS: OnceLock<Regex> = OnceLock::new();
    let def_re = re_cell(&DEF, r"^\s*(async\s+)?def\s+([A-Za-z_][A-Za-z0-9_]*)");
    let cls_re = re_cell(&CLS, r"^\s*class\s+([A-Za-z_][A-Za-z0-9_]*)");
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if let Some(c) = def_re.captures(line) {
            out.push(Sym { line: i, name: c[2].to_string(), kind: Kind::Fn });
        } else if let Some(c) = cls_re.captures(line) {
            out.push(Sym { line: i, name: c[1].to_string(), kind: Kind::Class });
        }
    }
    out
}

// ------------------------------------------------------------------ Go

fn scan_go(text: &str) -> Vec<Sym> {
    static FN: OnceLock<Regex> = OnceLock::new();
    static TY: OnceLock<Regex> = OnceLock::new();
    let fn_re = re_cell(&FN, r"^func\s+(?:\([^)]*\)\s*)?([A-Za-z_][A-Za-z0-9_]*)");
    let ty_re = re_cell(&TY, r"^type\s+([A-Za-z_][A-Za-z0-9_]*)\s+(struct|interface)");
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if let Some(c) = fn_re.captures(line) {
            out.push(Sym { line: i, name: c[1].to_string(), kind: Kind::Fn });
        } else if let Some(c) = ty_re.captures(line) {
            out.push(Sym { line: i, name: c[1].to_string(), kind: Kind::Class });
        }
    }
    out
}

// ------------------------------------------------------------------ Lua

fn scan_lua(text: &str) -> Vec<Sym> {
    static FN: OnceLock<Regex> = OnceLock::new();
    let fn_re = re_cell(&FN, r"^\s*(local\s+)?function\s+([A-Za-z_][A-Za-z0-9_.:]*)");
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if let Some(c) = fn_re.captures(line) {
            out.push(Sym { line: i, name: c[2].to_string(), kind: Kind::Fn });
        }
    }
    out
}

// ------------------------------------------------------------------ Ruby

fn scan_rb(text: &str) -> Vec<Sym> {
    static DEF: OnceLock<Regex> = OnceLock::new();
    static CLS: OnceLock<Regex> = OnceLock::new();
    let def_re = re_cell(&DEF, r"^\s*def\s+([A-Za-z_][A-Za-z0-9_?!]*)");
    let cls_re = re_cell(&CLS, r"^\s*(class|module)\s+([A-Za-z_][A-Za-z0-9_:]*)");
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if let Some(c) = def_re.captures(line) {
            out.push(Sym { line: i, name: c[1].to_string(), kind: Kind::Fn });
        } else if let Some(c) = cls_re.captures(line) {
            out.push(Sym { line: i, name: c[2].to_string(), kind: Kind::Class });
        }
    }
    out
}

// ------------------------------------------------------------------ JS / TS / PHP

fn scan_script(text: &str) -> Vec<Sym> {
    static FN: OnceLock<Regex> = OnceLock::new();
    static ARROW: OnceLock<Regex> = OnceLock::new();
    static CLS: OnceLock<Regex> = OnceLock::new();
    let fn_re = re_cell(&FN, r"^\s*(export\s+)?(default\s+)?(async\s+)?function\s*\*?\s*([A-Za-z_$][A-Za-z0-9_$]*)");
    let arrow_re = re_cell(&ARROW, r"^\s*(export\s+)?(const|let|var)\s+([A-Za-z_$][A-Za-z0-9_$]*)\s*(:[^=]+)?=\s*(async\s*)?(\([^)]*\)|[A-Za-z_$][A-Za-z0-9_$]*)\s*=>");
    let cls_re = re_cell(&CLS, r"^\s*(export\s+)?(default\s+)?(abstract\s+)?class\s+([A-Za-z_$][A-Za-z0-9_$]*)");
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if let Some(c) = fn_re.captures(line) {
            out.push(Sym { line: i, name: c[c.len() - 1].to_string(), kind: Kind::Fn });
        } else if let Some(c) = arrow_re.captures(line) {
            out.push(Sym { line: i, name: c[3].to_string(), kind: Kind::Fn });
        } else if let Some(c) = cls_re.captures(line) {
            out.push(Sym { line: i, name: c[c.len() - 1].to_string(), kind: Kind::Class });
        }
    }
    out
}

// ------------------------------------------------------------------ C-like (C/C++/C#/Java/…)

const CLIKE_CTRL: &[&str] = &[
    "if", "else", "for", "foreach", "while", "do", "switch", "case", "return", "catch",
    "sizeof", "using", "lock", "new", "delete", "throw", "elif", "synchronized", "with",
];

fn scan_clike(text: &str) -> Vec<Sym> {
    static CLS: OnceLock<Regex> = OnceLock::new();
    static CALL: OnceLock<Regex> = OnceLock::new();
    let cls_re = re_cell(&CLS, r"^\s*(pub\s+)?(export\s+)?(abstract\s+)?(final\s+)?(sealed\s+)?(static\s+)?(class|struct|interface|enum)\s+([A-Za-z_][A-Za-z0-9_]*)");
    let call_re = re_cell(&CALL, r"([A-Za-z_][A-Za-z0-9_]*)\s*\(");
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if let Some(c) = cls_re.captures(line) {
            out.push(Sym { line: i, name: c[c.len() - 1].to_string(), kind: Kind::Class });
            continue;
        }
        // method heuristic: the line ends with '{', has a name( … ) shape and
        // is not a control-flow statement or an assignment.
        let t = line.trim_end();
        if !t.ends_with('{') || t.contains(';') || !t.contains('(') {
            continue;
        }
        let head = t.trim_end_matches('{').trim_end();
        if head.contains('=') || head.starts_with('#') || head.starts_with('@') {
            continue;
        }
        let Some(cm) = call_re.captures(head) else { continue };
        let name = cm[1].to_string();
        let before = &head[..cm.get(1).unwrap().start()];
        let prev_word = before
            .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
            .filter(|s| !s.is_empty())
            .next_back()
            .unwrap_or("");
        if CLIKE_CTRL.contains(&name.as_str())
            || CLIKE_CTRL.contains(&prev_word)
            || name == "operator"
        {
            continue;
        }
        out.push(Sym { line: i, name, kind: Kind::Fn });
    }
    out
}

// ------------------------------------------------------------------ Markdown

fn scan_md(text: &str) -> Vec<Sym> {
    static H: OnceLock<Regex> = OnceLock::new();
    let h_re = re_cell(&H, r"^(#{1,4})\s+(.+?)\s*#*\s*$");
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if let Some(c) = h_re.captures(line) {
            out.push(Sym {
                line: i,
                name: c[2].to_string(),
                kind: Kind::Heading,
            });
        }
    }
    out
}

// ------------------------------------------------------------------ tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_fn_struct_trait() {
        let src = "pub fn alpha(x: u8) -> u8 {\n}\nstruct Beta;\nimpl Beta {\n    pub async fn gamma(&self) {}\n}\ntrait Delta {}\nfn _main() {}\n";
        let syms = scan("rs", src);
        let names: Vec<(String, Kind)> =
            syms.iter().map(|s| (s.name.clone(), s.kind)).collect();
        assert!(names.contains(&("alpha".into(), Kind::Fn)));
        assert!(names.contains(&("Beta".into(), Kind::Class)));
        assert!(names.contains(&("gamma".into(), Kind::Fn)));
        assert!(names.contains(&("Delta".into(), Kind::Class)));
        assert!(names.contains(&("_main".into(), Kind::Fn)));
        // lines are 0-based and point at the right rows
        assert_eq!(syms.iter().find(|s| s.name == "alpha").unwrap().line, 0);
        assert_eq!(syms.iter().find(|s| s.name == "gamma").unwrap().line, 4);
    }

    #[test]
    fn python_def_class() {
        let src = "class A:\n    def f(self):\n        pass\n\nasync def g():\n    pass\n";
        let syms = scan("py", src);
        assert_eq!(syms.len(), 3);
        assert_eq!(syms[0].kind, Kind::Class);
        assert_eq!(syms[1].name, "f");
        assert_eq!(syms[2].name, "g");
    }

    #[test]
    fn go_func_method_type() {
        let src = "func Sum(a int) int {\n}\nfunc (s *T) Len() int {\n}\ntype T struct {\n}\n";
        let syms = scan("go", src);
        let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["Sum", "Len", "T"]);
    }

    #[test]
    fn clike_methods_skip_control() {
        let src = "int main(void) {\n}\nvoid do_work(int a) {\n    if (a) {\n    }\n    while (x) {\n    }\n}\nclass Foo {\n};\n";
        let syms = scan("c", src);
        let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"main"));
        assert!(names.contains(&"do_work"));
        assert!(names.contains(&"Foo"));
        assert!(!names.contains(&"if") && !names.contains(&"while"));
    }

    #[test]
    fn js_arrow_and_class() {
        let src = "const add = (a, b) => a + b\nexport function huge() {\n}\nclass Widget {\n}\nlet f2 = async x => x\n";
        let syms = scan("js", src);
        let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"add"));
        assert!(names.contains(&"huge"));
        assert!(names.contains(&"Widget"));
        assert!(names.contains(&"f2"));
    }

    #[test]
    fn md_headings() {
        let src = "# Title\n\ntext\n## Sub *em*\n";
        let syms = scan("md", src);
        assert_eq!(syms.len(), 2);
        assert_eq!(syms[0].name, "Title");
        assert_eq!(syms[1].line, 3);
    }

    #[test]
    fn lua_and_php() {
        let l = scan("lua", "function M.helper() end\nlocal function priv() end\n");
        assert_eq!(l.len(), 2);
        let p = scan("php", "<?php\nclass A {\n  public function go() {\n  }\n}\nfunction top() {\n}\n");
        let names: Vec<&str> = p.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"A") && names.contains(&"top"));
    }
}
