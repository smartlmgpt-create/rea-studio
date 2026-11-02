//! Code suggestions (autocomplete) — pure Rust, no network.
//! Sources: per-language keyword tables + all words of the current document.
//! The popup behaves like VS Code's IntelliSense-lite: it appears while you
//! type a word (2+ chars), Up/Down navigates, Enter/Tab accepts, Esc closes.

use std::collections::HashSet;

/// Active suggestion popup state.
#[derive(Clone)]
pub struct Suggest {
    pub items: Vec<String>,
    pub sel: usize,
    /// the word currently being completed (as typed)
    pub base: String,
    /// popup anchor position on screen (below the caret)
    pub pos: egui::Pos2,
    /// document index the popup belongs to
    pub doc: usize,
    /// v1.6 Code Recommendations: snippet entries shown FIRST (◆ rows);
    /// selection index runs over snips then items.
    pub snips: Vec<crate::recco::Snip>,
}

impl Suggest {
    /// total selectable rows
    pub fn total(&self) -> usize {
        self.snips.len() + self.items.len()
    }
}

// ---------------------------------------------------------------- keyword tables

const JS: &[&str] = &[
    "async","await","break","case","catch","class","const","continue","debugger","default",
    "delete","do","else","export","extends","finally","for","function","if","import","in",
    "instanceof","let","new","null","of","return","static","super","switch","this","throw",
    "try","typeof","undefined","var","void","while","yield","true","false",
    "console.log","console.error","console.warn","document.querySelector","document.getElementById",
    "addEventListener","removeEventListener","appendChild","createElement","querySelectorAll",
    "setTimeout","setInterval","clearTimeout","clearInterval","fetch","Promise","JSON.parse",
    "JSON.stringify","Object.keys","Object.values","Object.entries","Array.isArray","Array.from",
    "map","filter","reduce","forEach","push","pop","shift","unshift","slice","splice","indexOf",
    "includes","find","findIndex","split","join","trim","replace","toLowerCase","toUpperCase",
    "parseInt","parseFloat","toString","length","Math.random","Math.floor","Math.ceil","Math.abs",
];
const PY: &[&str] = &[
    "and","as","assert","async","await","break","class","continue","def","del","elif","else",
    "except","finally","for","from","global","if","import","in","is","lambda","None","nonlocal",
    "not","or","pass","raise","return","True","False","try","while","with","yield","self",
    "print","len","range","enumerate","zip","map","filter","sorted","reversed","sum","min","max",
    "abs","round","str","int","float","list","dict","set","tuple","bool","type","isinstance",
    "getattr","setattr","hasattr","open","input","format","append","extend","pop","keys","values",
    "items","get","join","split","strip","replace","startswith","endswith","lower","upper",
    "super().__init__","if __name__ == '__main__'","__init__","__str__","__repr__",
];
const RUST: &[&str] = &[
    "as","async","await","break","const","continue","crate","dyn","else","enum","extern","false",
    "fn","for","if","impl","in","let","loop","match","mod","move","mut","pub","ref","return",
    "self","Self","static","struct","super","trait","true","type","unsafe","use","where","while",
    "println!","print!","eprintln!","format!","vec!","panic!","assert!","assert_eq!","write!",
    "writeln!","Some","None","Ok","Err","Result","Option","String","Vec","Box","Rc","Arc","RefCell",
    "Mutex","HashMap","HashSet","BTreeMap","clone","to_string","iter","into_iter","collect","map",
    "filter","unwrap","expect","push","pop","len","is_empty","as_str","to_owned","borrow","borrow_mut",
];
const GO: &[&str] = &[
    "break","case","chan","const","continue","default","defer","else","fallthrough","for","func",
    "go","goto","if","import","interface","map","package","range","return","select","struct",
    "switch","type","var","nil","true","false","fmt.Println","fmt.Printf","fmt.Sprintf","fmt.Errorf",
    "make","new","len","cap","append","copy","delete","panic","recover","close","error","string",
    "int","int64","float64","bool","byte","rune","defer func()",
];
const C: &[&str] = &[
    "auto","break","case","char","const","continue","default","do","double","else","enum","extern",
    "float","for","goto","if","inline","int","long","register","restrict","return","short","signed",
    "sizeof","static","struct","switch","typedef","union","unsigned","void","volatile","while",
    "printf","scanf","malloc","calloc","realloc","free","memcpy","memset","strlen","strcpy","strcmp",
    "fprintf","fopen","fclose","NULL","std","cout","cin","endl","vector","string","map","set",
    "namespace","using","class","template","public","private","protected","virtual","override",
    "nullptr","true","false","std::vector","std::string","std::map","std::cout","std::endl",
];
const CSHARP: &[&str] = &[
    "abstract","as","async","await","base","bool","break","byte","case","catch","char","checked",
    "class","const","continue","decimal","default","delegate","do","double","else","enum","event",
    "explicit","extern","false","finally","fixed","float","for","foreach","get","goto","if","implicit",
    "in","int","interface","internal","is","lock","long","namespace","new","null","object","operator",
    "out","override","params","private","protected","public","readonly","ref","return","sealed",
    "set","short","sizeof","static","string","struct","switch","this","throw","true","try","typeof",
    "uint","ulong","unchecked","unsafe","ushort","using","virtual","void","volatile","while","var",
    "Console.WriteLine","Console.ReadLine","ToString","ToList","AddRange","FirstOrDefault",
];
const JAVA: &[&str] = &[
    "abstract","assert","boolean","break","byte","case","catch","char","class","const","continue",
    "default","do","double","else","enum","extends","final","finally","float","for","goto","if",
    "implements","import","instanceof","int","interface","long","native","new","package","private",
    "protected","public","return","short","static","strictfp","super","switch","synchronized","this",
    "throw","throws","transient","try","void","volatile","while","true","false","null","var",
    "System.out.println","toString","equals","hashCode","ArrayList","HashMap","String","Integer",
];
const HTML: &[&str] = &[
    "html","head","body","div","span","a","p","h1","h2","h3","h4","h5","h6","ul","ol","li","table",
    "tr","td","th","form","input","button","select","option","textarea","label","img","video","audio",
    "canvas","svg","script","style","link","meta","title","header","footer","nav","main","section",
    "article","aside","iframe","br","hr","strong","em","code","pre","blockquote","class","id","href",
    "src","alt","width","height","charset","viewport","content","type","name","value","placeholder",
    "data-theme","data-src","lang","dir","rel","target",
];
const CSS: &[&str] = &[
    "color","background","background-color","background-image","font-size","font-family","font-weight",
    "font-style","line-height","letter-spacing","text-align","text-decoration","text-transform",
    "margin","margin-top","margin-bottom","margin-left","margin-right","padding","padding-top",
    "padding-bottom","padding-left","padding-right","border","border-radius","border-color",
    "border-width","width","height","min-width","max-width","min-height","max-height","display",
    "flex","grid","flex-direction","flex-wrap","justify-content","align-items","align-content",
    "gap","position","top","right","bottom","left","z-index","overflow","overflow-x","overflow-y",
    "opacity","transition","transform","box-shadow","cursor","pointer","outline","visibility",
    "white-space","word-wrap","float","clear","list-style","content","absolute","relative","fixed",
    "sticky","hidden","auto","none","center","space-between","space-around","column","row","wrap",
];
const SQL: &[&str] = &[
    "SELECT","FROM","WHERE","INSERT","INTO","VALUES","UPDATE","SET","DELETE","CREATE","TABLE",
    "ALTER","DROP","ADD","COLUMN","PRIMARY","KEY","FOREIGN","REFERENCES","JOIN","LEFT","RIGHT",
    "INNER","OUTER","FULL","ON","GROUP","BY","ORDER","ASC","DESC","LIMIT","OFFSET","HAVING",
    "COUNT","SUM","AVG","MIN","MAX","DISTINCT","AS","AND","OR","NOT","NULL","IS","IN","LIKE",
    "BETWEEN","EXISTS","CASE","WHEN","THEN","ELSE","END","UNION","ALL","INDEX","VIEW","IF",
];
const SH: &[&str] = &[
    "if","then","else","elif","fi","for","while","do","done","case","esac","function","return",
    "local","export","source","alias","echo","printf","read","cd","ls","cp","mv","rm","mkdir","rmdir",
    "touch","cat","grep","sed","awk","find","xargs","chmod","chown","curl","wget","tar","zip","unzip",
    "git","clone","pull","push","commit","status","branch","checkout","merge","rebase","log","diff",
    "npm","npx","node","python","python3","pip","cargo","rustc","go","docker","compose","make",
];
const PS1: &[&str] = &[
    "if","else","elseif","foreach","for","while","do","switch","function","return","param","begin",
    "process","end","try","catch","finally","throw","Write-Output","Write-Host","Write-Error",
    "Write-Warning","Get-ChildItem","Get-Content","Set-Content","Get-Item","Set-Item","Get-Location",
    "Set-Location","Get-Process","Stop-Process","Get-Service","Start-Service","Get-Command",
    "Get-Help","Get-Member","Select-Object","Where-Object","ForEach-Object","Sort-Object",
    "Measure-Object","Out-File","Out-Null","Invoke-WebRequest","Invoke-RestMethod","Expand-Archive",
    "Compress-Archive","Copy-Item","Move-Item","Remove-Item","New-Item","Test-Path","Join-Path",
    "Split-Path","Start-Process","Start-Job","$null","$true","$false","$env:PATH","$PSVersionTable",
];
const PHP: &[&str] = &[
    "abstract","and","array","as","break","callable","case","catch","class","clone","const","continue",
    "declare","default","do","echo","else","elseif","empty","enddeclare","endfor","endforeach",
    "endif","endswitch","endwhile","extends","final","finally","fn","for","foreach","function",
    "global","goto","if","implements","include","include_once","instanceof","insteadof","interface",
    "isset","list","namespace","new","or","print","private","protected","public","require",
    "require_once","return","static","switch","throw","trait","try","unset","use","var","while",
    "xor","yield","true","false","null","strlen","count","implode","explode","str_replace","preg_match",
];
const RUBY: &[&str] = &[
    "def","end","class","module","if","elsif","else","unless","case","when","while","until","for",
    "do","begin","rescue","ensure","raise","yield","return","self","nil","true","false","and","or",
    "not","require","require_relative","include","extend","attr_accessor","attr_reader","attr_writer",
    "puts","print","p","gets","each","map","select","reject","reduce","each_with_index","times",
    "new","lambda","proc","then","alias","super","initialize","to_s","to_i","to_a","length","size",
];
const LUA: &[&str] = &[
    "and","break","do","else","elseif","end","false","for","function","goto","if","in","local","nil",
    "not","or","repeat","return","then","true","until","while","print","pairs","ipairs","type",
    "tostring","tonumber","require","pcall","error","setmetatable","getmetatable","table.insert",
    "table.remove","table.concat","string.format","string.sub","string.find","string.gsub","math.floor",
];
const VUE_SVELTE: &[&str] = &[
    "template","script","style","setup","props","emit","ref","reactive","computed","watch","onMounted",
    "onUnmounted","defineProps","defineEmits","v-if","v-for","v-model","v-show","v-bind","v-on",
    "slot","component","export","default","data","methods","mounted","created","import","from",
];

pub fn keywords(ext: &str) -> &'static [&'static str] {
    match ext {
        "js" | "mjs" | "cjs" | "jsx" | "json" => JS,
        "ts" | "tsx" => JS,
        "py" | "pyw" => PY,
        "rs" => RUST,
        "go" => GO,
        "c" | "h" | "cpp" | "cc" | "cxx" | "hpp" | "hh" => C,
        "cs" => CSHARP,
        "java" => JAVA,
        "html" | "htm" | "xml" | "svg" | "vue" => HTML,
        "css" | "scss" | "sass" | "less" => CSS,
        "sql" => SQL,
        "sh" | "bash" | "zsh" | "bat" | "cmd" => SH,
        "ps1" | "psm1" => PS1,
        "php" => PHP,
        "rb" => RUBY,
        "lua" => LUA,
        "svelte" => VUE_SVELTE,
        _ => JS,
    }
}

// ---------------------------------------------------------------- word helpers

/// Word chars for completion purposes.
#[inline]
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Find the word (start,end) in CHAR indices around `cursor` (char index).
pub fn word_at(chars: &[char], cursor: usize) -> (usize, usize) {
    let cursor = cursor.min(chars.len());
    let mut start = cursor;
    while start > 0 && is_word_char(chars[start - 1]) {
        start -= 1;
    }
    let mut end = cursor;
    while end < chars.len() && is_word_char(chars[end]) {
        end += 1;
    }
    (start, end)
}

/// Collect unique words (len>=3) from the document, capped.
pub fn doc_words(text: &str, cap: usize) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for c in text.chars() {
        if is_word_char(c) {
            cur.push(c);
            if cur.len() > 40 {
                cur.clear(); // absurdly long token — not a useful suggestion
            }
        } else {
            if cur.chars().count() >= 3 && seen.insert(cur.to_lowercase()) {
                out.push(std::mem::take(&mut cur));
                if out.len() >= cap {
                    return out;
                }
            } else {
                cur.clear();
            }
        }
    }
    if cur.chars().count() >= 3 && seen.insert(cur.to_lowercase()) {
        out.push(cur);
    }
    out
}

/// Filter + rank suggestions for `prefix`. Longer prefix = fewer items.
/// Exact-word suppression is handled by the caller.
pub fn filter(kw: &'static [&'static str], words: &[String], prefix: &str, cap: usize) -> Vec<String> {
    let pl = prefix.to_lowercase();
    if pl.is_empty() {
        // Ctrl+Space with no word: show keywords first, then doc words
        let mut v: Vec<String> = kw.iter().take(cap).map(|s| s.to_string()).collect();
        for w in words {
            if v.len() >= cap {
                break;
            }
            v.push(w.clone());
        }
        v.truncate(cap);
        return v;
    }
    let mut scored: Vec<(i32, String)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for k in kw {
        let kl = k.to_lowercase();
        if kl.starts_with(&pl) {
            if seen.insert(kl.clone()) {
                let score = 1000 - kl.len() as i32 - if kl == pl { 0 } else { 2 };
                scored.push((score, k.to_string()));
            }
        }
    }
    for w in words {
        let wl = w.to_lowercase();
        if wl.starts_with(&pl) && seen.insert(wl.clone()) {
            let score = 500 - wl.len() as i32;
            scored.push((score, w.clone()));
        }
        if scored.len() > cap * 4 {
            break;
        }
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.len().cmp(&b.1.len())));
    scored.truncate(cap);
    scored.into_iter().map(|(_, s)| s).collect()
}
