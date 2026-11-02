//! Editing operations: bracket auto-pair, skip-over, selection wrap,
//! auto-indent, multi-occurrence editing and comment toggling.
//!
//! All functions take/return CHAR indices (egui CCursor space).

/// The closing partner of an opener (or the char itself for quotes).
pub fn pair_close(open: char) -> Option<char> {
    match open {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        '"' => Some('"'),
        '\'' => Some('\''),
        '`' => Some('`'),
        _ => None,
    }
}

pub fn is_opener(c: char) -> bool {
    matches!(c, '(' | '[' | '{')
}

pub fn is_quote(c: char) -> bool {
    matches!(c, '"' | '\'' | '`')
}

pub fn is_pair_char(c: char) -> bool {
    is_opener(c) || is_quote(c)
}

/// If the char just typed duplicates the char at the caret, we should
/// "skip over" instead of inserting a second one.
/// `cc` = caret AFTER TextEdit inserted the typed char.
/// Applies when typing a quote or a closer directly before the same char.
pub fn skip_over(text: &str, cc: usize, typed: char) -> Option<(String, usize)> {
    let closable = is_quote(typed) || matches!(typed, ')' | ']' | '}');
    if !closable {
        return None;
    }
    let chars: Vec<char> = text.chars().collect();
    if cc == 0 || cc >= chars.len() {
        return None;
    }
    // the just-inserted char (chars[cc-1]) must equal the ORIGINAL char (chars[cc])
    if chars[cc - 1] == typed && chars[cc] == typed {
        let mut out: String = chars[..cc - 1].iter().collect();
        out.extend(chars[cc..].iter());
        return Some((out, cc - 1));
    }
    None
}

/// Auto-close: after typing `open`, insert its partner right after the caret.
/// Returns (new_text, caret) — caret stays between the pair.
/// Suppressed when the next char is a word char (e.g. typing "(" before "abc").
pub fn auto_pair(text: &str, cc: usize, open: char) -> Option<(String, usize)> {
    let close = pair_close(open)?;
    let chars: Vec<char> = text.chars().collect();
    if cc > chars.len() {
        return None;
    }
    if let Some(&next) = chars.get(cc) {
        if next.is_alphanumeric() || next == '_' || next == close {
            return None; // typing inside a word or before an identical closer
        }
    }
    let mut out = String::with_capacity(text.len() + 2);
    out.extend(chars[..cc].iter());
    out.push(open);
    out.push(close);
    out.extend(chars[cc..].iter());
    Some((out, cc))
}

/// Wrap a selection with a pair — called BEFORE the typed char reached
/// TextEdit (handle_shortcuts consumes the event), so unlike `wrap_selection`
/// there is no "typed char right before the caret" precondition.
pub fn wrap_selection_direct(
    text: &str,
    sel: (usize, usize),
    open: char,
) -> Option<(String, usize, usize)> {
    let close = pair_close(open)?;
    let chars: Vec<char> = text.chars().collect();
    let (s, e) = if sel.0 <= sel.1 { (sel.0, sel.1) } else { (sel.1, sel.0) };
    if s >= e || e > chars.len() {
        return None;
    }
    let mut out = String::with_capacity(text.len() + 2);
    out.extend(chars[..s].iter());
    out.push(open);
    out.extend(chars[s..e].iter());
    out.push(close);
    out.extend(chars[e..].iter());
    Some((out, s, e + 2))
}

/// Wrap a selection with a pair (VS Code: select text, type `(`).
/// sel = (start, end) char indices BEFORE the typed char was inserted.
/// `cc` = caret after TextEdit inserted the char (selection collapsed to it).
/// We reconstruct: remove the typed char, then wrap the original selection.
pub fn wrap_selection(
    text: &str,
    sel: (usize, usize),
    cc: usize,
    open: char,
) -> Option<(String, usize, usize)> {
    let close = pair_close(open)?;
    let chars: Vec<char> = text.chars().collect();
    if cc == 0 || cc > chars.len() || chars[cc - 1] != open {
        return None; // the typed char isn't right before the caret
    }
    let (s, e) = if sel.0 <= sel.1 { (sel.0, sel.1) } else { (sel.1, sel.0) };
    if s >= e || e > chars.len() {
        return None;
    }
    let mut out = String::with_capacity(text.len() + 2);
    out.extend(chars[..s].iter());
    out.push(open);
    out.extend(chars[s..e].iter());
    out.push(close);
    out.extend(chars[e..].iter());
    Some((out, s, e + 2))
}

/// Smart indent AFTER an Enter was already inserted.
/// `cc` = caret position after the newline.
/// Cases:
///  * normal: copy previous line's indent to the new line
///  * prev line ends with an opener AND next char is its closer:
///    expand `{|}` into three lines (VS Code style)
///  * prev line ends with an opener: indent one more level
pub fn auto_indent(text: &str, cc: usize, tab_size: usize) -> Option<(String, usize)> {
    let chars: Vec<char> = text.chars().collect();
    if cc == 0 || cc > chars.len() || chars[cc - 1] != '\n' {
        return None;
    }
    // previous line content + indent
    let mut ls = cc - 1;
    while ls > 0 && chars[ls - 1] != '\n' {
        ls -= 1;
    }
    let prev_line: Vec<char> = chars[ls..cc - 1].to_vec();
    let base_indent: String = prev_line
        .iter()
        .take_while(|c| **c == ' ' || **c == '\t')
        .collect();
    let trimmed: String = prev_line.iter().skip(base_indent.len()).collect();
    let trimmed = trimmed.trim_end().to_string();
    let indent_unit = " ".repeat(tab_size.max(1));

    let last_ch = trimmed.chars().last();
    let next_ch = chars.get(cc).copied();

    // "indent then dedent" lines like `}` `)` `else:` … : prev line ends with ':'
    // and new line starts with a dedent keyword → same indent (no extra level)
    let dedent_kw = matches!(
        trimmed.as_str(),
        "else" | "else:" | "elif" | "except:" | "finally:" | "case" | "default:"
    ) || trimmed.starts_with("} else")
        || trimmed.starts_with(") else");

    if is_opener(last_ch.unwrap_or(' ')) {
        let close = pair_close(last_ch.unwrap_or(' ')).unwrap();
        if next_ch == Some(close) {
            // expand between pair:  {\n    \n}
            let inner = format!("{}{}", base_indent, indent_unit);
            let mut out = String::with_capacity(text.len() + inner.len() + base_indent.len() + 2);
            out.extend(chars[..cc].iter());
            out.push_str(&inner);
            out.push('\n');
            out.push_str(&base_indent);
            out.extend(chars[cc..].iter());
            let new_cc = cc + inner.chars().count();
            return Some((out, new_cc));
        }
        let inner = format!("{}{}", base_indent, indent_unit);
        let mut out = String::with_capacity(text.len() + inner.len() + 1);
        out.extend(chars[..cc].iter());
        out.push_str(&inner);
        out.extend(chars[cc..].iter());
        Some((out, cc + inner.chars().count()))
    } else {
        let ind = if dedent_kw {
            base_indent.clone()
        } else {
            base_indent.clone()
        };
        if ind.is_empty() {
            return None; // nothing to do
        }
        let mut out = String::with_capacity(text.len() + ind.len() + 1);
        out.extend(chars[..cc].iter());
        out.push_str(&ind);
        out.extend(chars[cc..].iter());
        Some((out, cc + ind.chars().count()))
    }
}

/// Find all occurrences of `needle` (literal) as char ranges, optionally
/// case-insensitive / whole-word. Capped.
pub fn find_all(text: &str, needle: &str, case_sensitive: bool, whole_word: bool, cap: usize) -> Vec<(usize, usize)> {
    if needle.is_empty() {
        return Vec::new();
    }
    if !case_sensitive {
        // to_lowercase() can change CHAR counts (e.g. 'İ' folds into 2 chars),
        // which shifts every reported range and corrupts later replacements.
        // Use the regex engine for case folding instead — find_all_regex maps
        // its byte offsets back to char space of the ORIGINAL text.
        let pat = if whole_word {
            format!(r"\b(?:{})\b", regex::escape(needle))
        } else {
            regex::escape(needle)
        };
        if let Ok(re) = regex::RegexBuilder::new(&pat)
            .case_insensitive(true)
            .size_limit(10_000_000)
            .build()
        {
            return find_all_regex(&re, text, cap);
        }
        // if the pattern fails to build, fall through to the plain scan below
    }
    let mut out = Vec::new();
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let hay: Vec<char> = text.chars().collect();
    let nee: Vec<char> = needle.chars().collect();
    let n = nee.len();
    if hay.len() < n {
        return out;
    }
    'outer: for i in 0..=(hay.len() - n) {
        for j in 0..n {
            if hay[i + j] != nee[j] {
                continue 'outer;
            }
        }
        if whole_word {
            let before_ok = i == 0 || !is_word(hay[i - 1]);
            let after_ok = i + n >= hay.len() || !is_word(hay[i + n]);
            if !before_ok || !after_ok {
                continue;
            }
        }
        // record as char range (start,end) — caller works in char space
        out.push((i, i + n));
        if out.len() >= cap {
            break;
        }
    }
    out
}

/// Regex variant returning char ranges.
pub fn find_all_regex(re: &regex::Regex, text: &str, cap: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut char_walk: Vec<(usize, usize)> = Vec::with_capacity(text.chars().count() + 1);
    {
        let mut ci = 0usize;
        for (bi, _) in text.char_indices() {
            char_walk.push((bi, ci)); // byte -> char
            ci += 1;
        }
        char_walk.push((text.len(), ci));
    }
    let byte_to_char = |b: usize| -> usize {
        match char_walk.binary_search_by_key(&b, |x| x.0) {
            Ok(i) => char_walk[i].1,
            Err(ins) => char_walk[ins.saturating_sub(1)].1,
        }
    };
    for m in re.find_iter(text) {
        if m.end() == m.start() {
            continue; // skip empty matches
        }
        out.push((byte_to_char(m.start()), byte_to_char(m.end())));
        if out.len() >= cap {
            break;
        }
    }
    out
}

/// Replace a char range with `with`, returning (new_text, new_caret).
pub fn replace_range(text: &str, range: (usize, usize), with: &str) -> (String, usize) {
    let chars: Vec<char> = text.chars().collect();
    let (s, e) = range;
    let (s, e) = (s.min(chars.len()), e.min(chars.len()).max(s));
    let mut out = String::with_capacity(text.len() + with.len());
    out.extend(chars[..s].iter());
    out.push_str(with);
    out.extend(chars[e..].iter());
    (out, s + with.chars().count())
}

/// Apply a replacement to EVERY range, from last to first (indices stay valid).
pub fn replace_all_ranges(text: &str, ranges: &[(usize, usize)], with: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = chars;
    let with_c: Vec<char> = with.chars().collect();
    let mut sorted: Vec<(usize, usize)> = ranges.to_vec();
    sorted.sort_by(|a, b| b.0.cmp(&a.0)); // descending
    for (s, e) in sorted {
        let (s, e) = (s.min(out.len()), e.min(out.len()).max(s));
        out.splice(s..e, with_c.iter().copied());
    }
    out.into_iter().collect()
}

/// Toggle a line comment for the given extension (`//`, `#`, `--` …).
/// Returns the new text of the LINE (without newline) or None.
pub fn toggle_line_comment(line: &str, ext: &str) -> Option<String> {
    let marker = match ext {
        "rs" | "c" | "h" | "cpp" | "cc" | "hpp" | "cs" | "java" | "js" | "mjs" | "cjs"
        | "jsx" | "ts" | "tsx" | "go" | "json" | "jsonc" | "php" | "scss" | "less"
        | "swift" | "kt" | "kts" | "zig" | "dart" | "scala" => "//",
        "py" | "pyw" | "rb" | "sh" | "bash" | "zsh" | "yaml" | "yml" | "toml" | "r"
        | "perl" | "pl" | "makefile" | "mk" | "ps1" => "#",
        "lua" | "sql" | "hs" => "--",
        "html" | "htm" | "xml" | "svg" | "vue" => "", // handled as block below
        "css" => "",
        "erb" | "md" => "",
        _ => "//",
    };
    if marker.is_empty() {
        return None; // block-comment languages: not toggled line-wise here
    }
    let trimmed = line.trim_start();
    if trimmed.starts_with(marker) {
        // remove first occurrence of marker (+ one following space)
        let idx = line.find(marker).unwrap();
        let mut rest = &line[idx + marker.len()..];
        if let Some(r) = rest.strip_prefix(' ') {
            rest = r;
        } else if let Some(r) = rest.strip_prefix('\t') {
            rest = r;
        }
        Some(format!("{}{}", &line[..idx], rest))
    } else if !trimmed.is_empty() {
        let indent = line.len() - trimmed.len();
        Some(format!("{}{} {}", &line[..indent], marker, trimmed))
    } else {
        None
    }
}

// ---------------------------------------------------------------- matcher

/// Unified literal/regex matcher used by find-in-file.
pub enum Matcher2 {
    Literal { needle: String, cs: bool },
    Regex(regex::Regex),
}

impl Matcher2 {
    /// Find up to `cap` matches as flat CHAR ranges.
    pub fn find_all(&self, text: &str, cap: usize) -> Vec<(usize, usize)> {
        match self {
            Matcher2::Literal { needle, cs } => find_all(text, needle, *cs, false, cap),
            Matcher2::Regex(re) => find_all_regex(re, text, cap),
        }
    }
}

/// Cheap "symbol" for the breadcrumb bar: first `fn/def/class/…` on the line.
pub fn line_symbol(line: &str) -> Option<String> {
    let t = line.trim_start();
    for kw in [
        "fn ", "def ", "class ", "function ", "func ", "void ", "public ", "private ",
        "impl ", "struct ", "enum ", "trait ", "interface ", "type ", "module ", "sub ",
    ] {
        if let Some(rest) = t.strip_prefix(kw) {
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '!')
                .collect();
            if !name.is_empty() {
                let kw = kw.trim();
                return Some(format!("{kw} {name}"));
            }
        }
    }
    None
}
