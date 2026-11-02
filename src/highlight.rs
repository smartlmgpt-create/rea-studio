use eframe::egui;
use egui::text::LayoutJob;
use egui::{Color32, FontId, TextFormat};
use std::cell::RefCell;

use syntect::highlighting::{
    FontStyle, HighlightIterator, HighlightState, Highlighter as SynHighlighter, ThemeSet,
};
use syntect::parsing::{ParseState, ScopeStack, SyntaxReference, SyntaxSet};

/// Syntax highlighting built on syntect, with a small built-in fallback
/// highlighter for COBOL / ASM / NASM (not included in syntect's default set).
///
/// v1.5 (#18): per-line highlight cache. Parsing the WHOLE file with syntect
/// on every keystroke made ~5k-line files lag. We now keep, per line, the
/// styled spans AND the parser state at the line end; on each keystroke only
/// the lines from the first edit onward are re-parsed (block comments etc.
/// still propagate correctly because the state is restored).
pub struct Highlighter {
    pub ss: SyntaxSet,
    ts: ThemeSet,
    cache: RefCell<HighlightCache>,
}

/// one styled run of a line
struct Span {
    color: Color32,
    italics: bool,
    underline: bool,
    text: String,
}

struct CachedLine {
    spans: Vec<Span>,
    /// hash of the raw line text (cheap change detection, no per-frame allocs)
    hash: u64,
    /// parser + highlight state AFTER this line (restored for incremental
    /// re-highlighting — both are needed, e.g. for block comments)
    parse_state: ParseState,
    hl_state: HighlightState,
}

#[derive(Default)]
struct HighlightCache {
    lines: Vec<CachedLine>,
    /// validity key: (ext, dark, theme)
    key: (String, bool, &'static str),
}

#[derive(Clone, Copy, PartialEq)]
enum Fallback {
    Cobol,
    Asm,
}

const ASM_KW: &[&str] = &[
    "mov","movsx","movzx","push","pop","add","sub","mul","imul","div","idiv","inc","dec","jmp",
    "jz","jnz","je","jne","jl","jg","jle","jge","jb","ja","jbe","jae","js","jns","jo","jno",
    "jp","jnp","jc","jnc","loop","loope","loopne","call","ret","retn","int","iret","cmp","test",
    "and","or","xor","not","neg","shl","shr","sal","sar","rol","ror","rcl","rcr","lea","nop",
    "hlt","enter","leave","pushf","popf","sahf","lahf","xchg","cdq","cwd","cbw","stosb","stosw",
    "stosd","lodsb","lodsw","lodsd","movsb","movsw","movsd","scasb","scasw","rep","repe","repne",
    "cld","std","cli","sti","clc","stc","cmc","setz","setnz","setg","setl","endp","proc","local",
];
const ASM_REG: &[&str] = &[
    "eax","ebx","ecx","edx","esi","edi","esp","ebp","eip","ax","bx","cx","dx","al","ah","bl","bh",
    "cl","ch","si","di","sp","bp","rax","rbx","rcx","rdx","rsi","rdi","rsp","rbp","rip","r8","r9",
    "r10","r11","r12","r13","r14","r15","r8d","r9d","r10d","r11d","r12d","r13d","r14d","r15d",
    "xmm0","xmm1","xmm2","xmm3","xmm4","xmm5","xmm6","xmm7","cs","ds","es","fs","gs","ss",
];
const ASM_DIR: &[&str] = &[
    "section","segment","global","extern","bits","org","times","equ","db","dw","dd","dq","dt",
    "resb","resw","resd","resq","align","byte","word","dword","qword","ptr","offset","flat",
    "stdcall","use32","use64","include","macro","endmacro","struc","endstruc","common","end",
];
const COBOL_KW: &[&str] = &[
    "identification","division","program-id","author","environment","configuration",
    "source-computer","object-computer","special-names","data","file","file-control","select",
    "assign","working-storage","linkage","procedure","section","paragraph","accept","add","alter",
    "call","cancel","close","compute","continue","delete","display","divide","else","end-add",
    "end-compute","end-divide","end-evaluate","end-if","end-multiply","end-perform","end-read",
    "end-string","end-subtract","end-unstring","end-write","evaluate","exit","goback","go",
    "if","initialize","inspect","merge","move","multiply","next","open","perform","read","release",
    "return","rewrite","search","set","sort","start","stop","string","subtract","then","to",
    "unstring","until","when","write","varying","by","giving","pic","picture","value","values",
    "usage","comp-3","comp","binary","packed-decimal","occurs","times","indexing","ascending",
    "descending","key","redefines","filler","constant","function","end-program","declaratives",
    "end","not","and","or","greater","less","equal","than","size","errors","upon","with","no",
    "advancing","after","before","line","thru","from","tallying","converting","replacing",
    "delimited","into","pointer","true","false","other","invalid","at",
];

impl Highlighter {
    pub fn new() -> Self {
        Self {
            ss: SyntaxSet::load_defaults_newlines(),
            ts: ThemeSet::load_defaults(),
            cache: RefCell::default(),
        }
    }

    /// Display name for the status bar — 55+ languages.
    pub fn lang_name(ext: &str) -> &'static str {
        match ext {
            "js" | "mjs" | "cjs" | "jsx" => "JavaScript",
            "ts" | "tsx" => "TypeScript",
            "py" | "pyw" => "Python",
            "rs" => "Rust",
            "go" => "Go",
            "c" => "C",
            "h" => "C Header",
            "cpp" | "cc" | "cxx" | "hpp" | "hh" => "C++",
            "m" | "mm" => "Objective-C",
            "cs" => "C#",
            "java" => "Java",
            "kt" | "kts" => "Kotlin",
            "swift" => "Swift",
            "dart" => "Dart",
            "rb" => "Ruby",
            "php" => "PHP",
            "lua" => "Lua",
            "pl" | "pm" => "Perl",
            "r" => "R",
            "scala" => "Scala",
            "hs" => "Haskell",
            "ml" | "mli" => "OCaml",
            "clj" | "cljs" | "edn" => "Clojure",
            "ex" | "exs" => "Elixir",
            "erl" | "hrl" => "Erlang",
            "fs" | "fsi" | "fsx" => "F#",
            "groovy" | "gradle" => "Groovy",
            "vb" => "Visual Basic",
            "pas" | "pp" | "dpr" => "Pascal",
            "f90" | "f95" | "for" | "f" => "Fortran",
            "d" => "D",
            "asm" | "nasm" | "s" => "Assembly",
            "cob" | "cobol" | "cbl" => "COBOL",
            "zig" => "Zig",
            "nim" => "Nim",
            "jl" => "Julia",
            "cr" => "Crystal",
            "sh" | "bash" | "zsh" => "Shell",
            "fish" => "Fish Shell",
            "ps1" | "psm1" => "PowerShell",
            "bat" | "cmd" => "Batch",
            "sql" => "SQL",
            "html" | "htm" => "HTML",
            "xml" | "xsl" | "plist" => "XML",
            "svg" => "SVG",
            "css" => "CSS",
            "scss" | "sass" => "SCSS",
            "less" => "Less",
            "json" => "JSON",
            "yml" | "yaml" => "YAML",
            "toml" => "TOML",
            "ini" | "cfg" | "conf" | "properties" => "Config",
            "md" | "markdown" => "Markdown",
            "mk" | "mak" | "make" => "Makefile",
            "cmake" => "CMake",
            "tex" | "sty" | "cls" => "LaTeX",
            "diff" | "patch" => "Diff",
            "vue" => "Vue",
            "svelte" => "Svelte",
            "graphql" | "gql" => "GraphQL",
            "proto" => "Protocol Buffers",
            "coffee" => "CoffeeScript",
            "haml" => "Haml",
            "vim" => "Vim Script",
            "csv" | "tsv" => "CSV Data",
            "log" => "Log",
            _ => "Plain Text",
        }
    }

    fn fallback_for(ext: &str) -> Option<Fallback> {
        match ext {
            "cob" | "cobol" | "cbl" => Some(Fallback::Cobol),
            "asm" | "nasm" | "s" => Some(Fallback::Asm),
            _ => None,
        }
    }

    fn syntax(&self, ext: &str) -> Option<&SyntaxReference> {
        // explicit syntect syntax names (50+), then extension fallback which
        // resolves many more languages from syntect's default set
        let name = match ext {
            "js" | "mjs" | "cjs" | "jsx" => "JavaScript",
            "ts" | "tsx" => "TypeScript",
            "py" | "pyw" => "Python",
            "rs" => "Rust",
            "go" => "Go",
            "c" | "h" => "C",
            "cpp" | "cc" | "cxx" | "hpp" | "hh" => "C++",
            "m" | "mm" => "Objective-C",
            "cs" => "C#",
            "java" => "Java",
            "rb" => "Ruby",
            "php" => "PHP",
            "lua" => "Lua",
            "pl" | "pm" => "Perl",
            "r" => "R",
            "scala" => "Scala",
            "hs" => "Haskell",
            "ml" | "mli" => "OCaml",
            "clj" | "cljs" | "edn" => "Clojure",
            "erl" | "hrl" => "Erlang",
            "groovy" => "Groovy",
            "vb" => "Visual Basic",
            "pas" | "pp" | "dpr" => "Pascal",
            "d" => "D",
            "sh" | "bash" | "zsh" => "Bourne Again Shell (bash)",
            "bat" | "cmd" => "Batch File",
            "sql" => "SQL",
            "html" | "htm" => "HTML",
            "xml" | "xsl" | "svg" | "plist" => "XML",
            "css" => "CSS",
            "json" => "JSON",
            "yml" | "yaml" => "YAML",
            "toml" => "TOML",
            "properties" => "Java Properties",
            "md" | "markdown" => "Markdown",
            "mk" | "mak" | "make" => "Makefile",
            "tex" | "sty" | "cls" => "LaTeX",
            "diff" | "patch" => "Diff",
            "haml" => "Ruby Haml",
            "as" => "ActionScript",
            "lisp" | "cl" | "el" => "Lisp",
            _ => "",
        };
        if name.is_empty() {
            self.ss.find_syntax_by_extension(ext)
        } else {
            self.ss
                .find_syntax_by_name(name)
                .or_else(|| self.ss.find_syntax_by_extension(ext))
        }
    }

    /// Build the highlighted LayoutJob used by the editor's TextEdit layouter.
    /// v1.5: uses the incremental per-line cache (see struct docs).
    /// v1.9: `size` — the editor font size (settings × zoom) so the text and
    /// the row-height math stay in sync (the old hard-coded 14.0 made the
    /// font-size setting a no-op inside the editor).
    pub fn job(&self, text: &str, wrap_width: f32, dark: bool, ext: &str, size: f32) -> LayoutJob {
        let mono = FontId::monospace(size);
        let base_color = if dark {
            Color32::from_rgb(212, 212, 212)
        } else {
            Color32::from_rgb(32, 33, 36)
        };
        let mut job = LayoutJob::default();
        job.wrap.max_width = wrap_width;

        if text.is_empty() {
            return job;
        }

        if let Some(fb) = Self::fallback_for(ext) {
            self.fallback_job(&mut job, text, mono, base_color, fb);
            return job;
        }

        let st_name = if dark { "base16-ocean.dark" } else { "InspiredGitHub" };
        let pair = self.ts.themes.get(st_name).and_then(|theme| {
            self.syntax(ext).map(|syntax| (theme, syntax))
        });

        let Some((theme, syntax)) = pair else {
            job.append(text, 0.0, TextFormat::simple(mono.clone(), base_color));
            return job;
        };

        // ---------------- incremental cache ----------------
        let mut cache = self.cache.borrow_mut();
        if cache.key != (ext.to_string(), dark, st_name) {
            cache.key = (ext.to_string(), dark, st_name);
            cache.lines.clear();
        }
        let new_lines: Vec<&str> = syntect::util::LinesWithEndings::from(text).collect();
        // first line whose content changed — everything from here is re-parsed
        let mut first_changed = new_lines.len();
        for (i, nl) in new_lines.iter().enumerate() {
            match cache.lines.get(i) {
                // cheap validity check: line hash (no string building per frame)
                Some(c) if c.hash == crate::util::fnv(nl) => {}
                _ => {
                    first_changed = i;
                    break;
                }
            }
        }
        if first_changed < new_lines.len() || cache.lines.len() != new_lines.len() {
            // restore BOTH states from before the first changed line
            let highlighter = SynHighlighter::new(theme);
            let (mut parse_state, mut hl_state) = if first_changed > 0 {
                match cache.lines.get(first_changed - 1) {
                    Some(prev) => (prev.parse_state.clone(), prev.hl_state.clone()),
                    None => (
                        ParseState::new(syntax),
                        HighlightState::new(&highlighter, ScopeStack::new()),
                    ),
                }
            } else {
                (
                    ParseState::new(syntax),
                    HighlightState::new(&highlighter, ScopeStack::new()),
                )
            };
            cache.lines.truncate(first_changed);
            for &line in &new_lines[first_changed..] {
                let lhash = crate::util::fnv(line);
                let ops = match parse_state.parse_line(line, &self.ss) {
                    Ok(o) => o,
                    Err(_) => {
                        let spans = vec![Span {
                            color: base_color,
                            italics: false,
                            underline: false,
                            text: line.to_string(),
                        }];
                        cache.lines.push(CachedLine {
                            spans,
                            hash: lhash,
                            parse_state: parse_state.clone(),
                            hl_state: hl_state.clone(),
                        });
                        continue;
                    }
                };
                let mut spans = Vec::new();
                for (style, chunk) in
                    HighlightIterator::new(&mut hl_state, &ops, line, &highlighter)
                {
                    let fg = Color32::from_rgb(
                        style.foreground.r,
                        style.foreground.g,
                        style.foreground.b,
                    );
                    spans.push(Span {
                        color: fg,
                        italics: style.font_style.contains(FontStyle::ITALIC),
                        underline: style.font_style.contains(FontStyle::UNDERLINE),
                        text: chunk.to_string(),
                    });
                }
                cache.lines.push(CachedLine {
                    spans,
                    hash: lhash,
                    parse_state: parse_state.clone(),
                    hl_state: hl_state.clone(),
                });
            }
        }
        // rebuild the job from the (cached) spans — no re-parsing here
        for cl in cache.lines.iter() {
            for s in &cl.spans {
                let mut fmt = TextFormat::simple(mono.clone(), s.color);
                fmt.italics = s.italics;
                fmt.underline = if s.underline {
                    egui::Stroke::new(1.0, s.color)
                } else {
                    egui::Stroke::NONE
                };
                job.append(&s.text, 0.0, fmt);
            }
        }
        job
    }

    /// Lightweight keyword highlighter for COBOL / ASM / NASM.
    fn fallback_job(
        &self,
        job: &mut LayoutJob,
        text: &str,
        mono: FontId,
        base: Color32,
        fb: Fallback,
    ) {
        let kw_col = Color32::from_rgb(86, 156, 214);
        let reg_col = Color32::from_rgb(156, 220, 254);
        let dir_col = Color32::from_rgb(197, 134, 192);
        let num_col = Color32::from_rgb(181, 206, 168);
        let str_col = Color32::from_rgb(206, 145, 120);
        let com_col = Color32::from_rgb(106, 115, 125);

        for line in text.split_inclusive('\n') {
            let is_comment = {
                let t = line.trim_start();
                match fb {
                    Fallback::Cobol => {
                        t.starts_with('*') || t.starts_with('/')
                    }
                    Fallback::Asm => t.starts_with(';'),
                }
            };
            if is_comment {
                push(job, line, &mono, com_col);
                continue;
            }

            // tokenize: strings, numbers, words, other
            let bytes = line.as_bytes();
            let mut i = 0usize;
            let mut plain_start = 0usize;
            let quote: char = match fb {
                Fallback::Cobol => '"',
                Fallback::Asm => ';', // asm strings use ' or "; handle both below
            };
            let _ = quote;

            while i < bytes.len() {
                let ch = line[i..].chars().next().unwrap();
                match ch {
                    '"' | '\'' => {
                        if plain_start < i {
                            push(job, &line[plain_start..i], &mono, base);
                        }
                        let start = i;
                        i += ch.len_utf8();
                        while i < bytes.len() {
                            let c2 = line[i..].chars().next().unwrap();
                            i += c2.len_utf8();
                            if c2 == ch {
                                break;
                            }
                        }
                        push(job, &line[start..i.min(line.len())], &mono, str_col);
                        plain_start = i;
                    }
                    'a'..='z' | 'A'..='Z' | '_' | '-' => {
                        if plain_start < i {
                            push(job, &line[plain_start..i], &mono, base);
                        }
                        let start = i;
                        while i < bytes.len() {
                            let c2 = line[i..].chars().next().unwrap();
                            if c2.is_ascii_alphanumeric() || c2 == '_' || c2 == '-' {
                                i += c2.len_utf8();
                            } else {
                                break;
                            }
                        }
                        let word = &line[start..i];
                        let lw = word.to_ascii_lowercase();
                        let col = if lw == "section" || lw == "global" || lw == "extern"
                            || lw == "division" || lw == "section" || lw.ends_with("-id")
                        {
                            dir_col
                        } else if match_list(&lw, ASM_KW) || match_list(&lw, COBOL_KW) {
                            kw_col
                        } else if match_list(&lw, ASM_REG) {
                            reg_col
                        } else if match_list(&lw, ASM_DIR) {
                            dir_col
                        } else {
                            base
                        };
                        push(job, word, &mono, col);
                        plain_start = i;
                    }
                    '0'..='9' => {
                        if plain_start < i {
                            push(job, &line[plain_start..i], &mono, base);
                        }
                        let start = i;
                        while i < bytes.len() {
                            let c2 = line[i..].chars().next().unwrap();
                            if c2.is_ascii_alphanumeric() || c2 == '.' || c2 == 'x' || c2 == 'h' {
                                i += c2.len_utf8();
                            } else {
                                break;
                            }
                        }
                        push(job, &line[start..i], &mono, num_col);
                        plain_start = i;
                    }
                    _ => {
                        i += ch.len_utf8();
                    }
                }
            }
            if plain_start < line.len() {
                push(job, &line[plain_start..], &mono, base);
            }
        }
    }
}

fn match_list(w: &str, list: &[&str]) -> bool {
    list.binary_search(&w).is_ok() || list.contains(&w)
}

fn push(job: &mut LayoutJob, text: &str, mono: &FontId, color: Color32) {
    if !text.is_empty() {
        job.append(text, 0.0, TextFormat::simple(mono.clone(), color));
    }
}
