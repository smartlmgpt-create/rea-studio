//! PLUGIN: Code Recommendations (v1.6)
//!
//! Offline snippet/template engine for 14 languages:
//! Python, HTML, CSS, JavaScript, Go, Rust, COBOL, PHP, Ruby, Swift,
//! Perl, C#, Kotlin, F#.
//!
//! Snippets are inserted through the suggestion popup (◆ rows) or via
//! Ctrl+Space with an empty prefix. `@@` inside `code` marks where the
//! caret lands after insertion.

/// One recommendation: a small, idiomatic code template.
#[derive(Clone)]
pub struct Snip {
    /// trigger word — matched against the word under the caret
    pub trig: &'static str,
    /// popup label (short)
    pub label: &'static str,
    /// popup description (long)
    pub desc: &'static str,
    /// template; `@@` = final caret position
    pub code: &'static str,
}

macro_rules! snips {
    ($($trig:expr => $label:expr, $desc:expr, $code:expr);* $(;)?) => {
        &[$( Snip { trig: $trig, label: $label, desc: $desc, code: $code } ),*]
    };
}

// ---------------------------------------------------------------- Python
const PY: &[Snip] = snips! {
    "def" => "def function()", "Function definition",
        "def @@():\n    pass\n";
    "main" => "if __name__ main", "Script entry point",
        "def main():\n    @@\n\n\nif __name__ == \"__main__\":\n    main()\n";
    "class" => "class", "Class with __init__",
        "class @@:\n    def __init__(self):\n        pass\n";
    "forr" => "for range loop", "for i in range(...)",
        "for i in range(@@):\n    pass\n";
    "fore" => "for each", "for item in iterable",
        "for item in @@:\n    pass\n";
    "try" => "try / except", "Exception handling",
        "try:\n    @@\nexcept Exception as e:\n    print(f\"Error: {e}\")\n";
    "with" => "with open file", "Read a file safely",
        "with open(\"@@\", \"r\", encoding=\"utf-8\") as f:\n    content = f.read()\n";
    "req" => "requests GET", "HTTP GET with requests",
        "import requests\n\nresp = requests.get(\"@@\", timeout=10)\nresp.raise_for_status()\nprint(resp.json())\n";
    "fastapi" => "FastAPI app", "Minimal FastAPI server",
        "from fastapi import FastAPI\n\napp = FastAPI()\n\n@app.get(\"/\")\ndef root():\n    return {\"status\": \"ok\"}\n";
    "test" => "pytest test", "Unit test skeleton",
        "def test_@@():\n    assert True\n";
    "datac" => "dataclass", "@dataclass definition",
        "from dataclasses import dataclass\n\n@dataclass\nclass @@:\n    name: str\n    value: int = 0\n";
    "async" => "async def", "Async function",
        "async def @@():\n    await asyncio.sleep(0)\n";
    "lambda" => "lambda", "Inline lambda",
        "lambda x: @@";
    "comp" => "list comprehension", "[x for x in ...]",
        "[x for x in @@ if x]"
};

// ---------------------------------------------------------------- HTML
const HTML: &[Snip] = snips! {
    "html5" => "HTML5 skeleton", "Full document boilerplate",
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n  <meta charset=\"UTF-8\">\n  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">\n  <title>@@</title>\n  <link rel=\"stylesheet\" href=\"style.css\">\n</head>\n<body>\n  <script src=\"script.js\"></script>\n</body>\n</html>\n";
    "div" => "<div class>", "Division with class",
        "<div class=\"@@\"></div>";
    "img" => "<img>", "Image with alt",
        "<img src=\"@@\" alt=\"description\">";
    "link" => "<a href>", "Hyperlink",
        "<a href=\"@@\">text</a>";
    "table" => "<table>", "Table skeleton",
        "<table>\n  <thead>\n    <tr><th>@@</th></tr>\n  </thead>\n  <tbody>\n    <tr><td>data</td></tr>\n  </tbody>\n</table>";
    "form" => "<form>", "Form with submit",
        "<form action=\"@@\" method=\"post\">\n  <input type=\"text\" name=\"value\" placeholder=\"value\">\n  <button type=\"submit\">Send</button>\n</form>";
    "input" => "<input>", "Labeled input field",
        "<label for=\"@@\">Label</label>\n<input type=\"text\" id=\"@@\" name=\"@@\">";
    "btn" => "<button>", "Button",
        "<button type=\"button\" onclick=\"@@\">Click</button>";
    "list" => "<ul> list", "Unordered list",
        "<ul>\n  <li>@@</li>\n  <li>item</li>\n</ul>";
    "script" => "<script>", "Inline script tag",
        "<script>\n  @@\n</script>";
    "style" => "<style>", "Inline style block",
        "<style>\n  @@\n</style>";
    "meta" => "<meta viewport>", "Responsive viewport tag",
        "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">";
    "video" => "<video>", "Video player",
        "<video src=\"@@\" controls></video>";
    "svg" => "<svg>", "Inline SVG icon",
        "<svg width=\"24\" height=\"24\" viewBox=\"0 0 24 24\" fill=\"none\">\n  <path d=\"@@\" stroke=\"currentColor\"/>\n</svg>"
};

// ---------------------------------------------------------------- CSS
const CSS: &[Snip] = snips! {
    "flex" => "flexbox center", "Center children with flex",
        "display: flex;\njustify-content: center;\nalign-items: center;\ngap: @@px;";
    "grid" => "grid layout", "Responsive grid",
        "display: grid;\ngrid-template-columns: repeat(auto-fill, minmax(220px, 1fr));\ngap: @@px;";
    "anim" => "keyframes", "Animation block",
        "@keyframes slide {\n  from { transform: translateX(-100%); }\n  to   { transform: translateX(0); }\n}\n\n.el {\n  animation: slide 0.3s ease @@;\n}";
    "media" => "media query", "Responsive breakpoint",
        "@media (max-width: @@px) {\n  \n}";
    "shadow" => "box-shadow", "Card shadow",
        "box-shadow: 0 4px 12px rgba(0, 0, 0, @@);";
    "grad" => "gradient", "Linear gradient background",
        "background: linear-gradient(@@deg, #667eea, #764ba2);";
    "round" => "border-radius", "Rounded corners",
        "border-radius: @@px;";
    "transition" => "transition", "Smooth hover transition",
        "transition: all 0.2s ease@@;";
    "center" => "absolute center", "Absolute centering",
        "position: absolute;\ntop: 50%;\nleft: 50%;\ntransform: translate(-50%, -50%);@@";
    "reset" => "css reset", "Minimal reset",
        "*, *::before, *::after {\n  box-sizing: border-box;\n  margin: 0;\n  padding: 0;\n}@@";
    "var" => "custom property", ":root variable",
        ":root {\n  --accent: @@;\n}";
    "scrollbar" => "custom scrollbar", "WebKit scrollbar styling",
        "::-webkit-scrollbar {\n  width: 8px;\n}\n::-webkit-scrollbar-thumb {\n  background: @@;\n  border-radius: 4px;\n}";
    "clamp" => "fluid font-size", "clamp() responsive text",
        "font-size: clamp(1rem, 2.5vw, @@rem);";
    "truncate" => "text truncate", "Single-line ellipsis",
        "white-space: nowrap;\noverflow: hidden;\ntext-overflow: ellipsis;@@"
};

// ---------------------------------------------------------------- JavaScript
const JS: &[Snip] = snips! {
    "fn" => "function", "Named function",
        "function @@() {\n  \n}";
    "afn" => "arrow function", "const arrow function",
        "const @@ = () => {\n  \n};";
    "fetch" => "fetch GET", "async fetch with json",
        "const res = await fetch(\"@@\");\nconst data = await res.json();\nconsole.log(data);";
    "axios" => "axios request", "axios with error handling",
        "try {\n  const { data } = await axios.get(\"@@\");\n  console.log(data);\n} catch (err) {\n  console.error(err);\n}";
    "promise" => "new Promise", "Promise executor",
        "new Promise((resolve, reject) => {\n  @@\n});";
    "aclass" => "class", "ES class with constructor",
        "class @@ {\n  constructor() {\n    \n  }\n}";
    "dom" => "querySelector", "DOM selection + listener",
        "const el = document.querySelector(\"@@\");\nel.addEventListener(\"click\", () => {\n  \n});";
    "map" => "array map", "Transform an array",
        "const out = items.map((item) => @@);";
    "tryc" => "try / catch", "Error handling",
        "try {\n  @@\n} catch (err) {\n  console.error(err);\n}";
    "timeout" => "setTimeout", "Delayed execution",
        "setTimeout(() => {\n  @@\n}, 1000);";
    "module" => "ES module", "Export / import pair",
        "export function @@() {\n  \n}";
    "localStorage" => "localStorage", "Save + load JSON",
        "localStorage.setItem(\"@@\", JSON.stringify(value));\nconst v = JSON.parse(localStorage.getItem(\"@@\") ?? \"null\");";
    "debounce" => "debounce", "Debounced function",
        "function debounce(fn, ms = 300) {\n  let t;\n  return (...args) => {\n    clearTimeout(t);\n    t = setTimeout(() => fn(...args), ms);\n  };\n}@@";
    "app" => "Express server", "Minimal express app",
        "import express from \"express\";\n\nconst app = express();\napp.use(express.json());\n\napp.get(\"/\", (req, res) => {\n  res.json({ status: \"ok\" });\n});\n\napp.listen(@@3000, () => console.log(\"running\"));"
};

// ---------------------------------------------------------------- Go
const GO: &[Snip] = snips! {
    "main" => "package main", "Executable entry point",
        "package main\n\nimport \"fmt\"\n\nfunc main() {\n\t@@\n}";
    "fn" => "func", "Function",
        "func @@() error {\n\treturn nil\n}";
    "struct" => "struct", "Struct type",
        "type @@ struct {\n\tName string\n\tAge  int\n}";
    "iface" => "interface", "Interface type",
        "type @@ interface {\n\tDo() error\n}";
    "err" => "error check", "if err != nil",
        "if err != nil {\n\treturn fmt.Errorf(\"@@: %w\", err)\n}";
    "http" => "http server", "net/http server",
        "mux := http.NewServeMux()\nmux.HandleFunc(\"/\", func(w http.ResponseWriter, r *http.Request) {\n\tfmt.Fprintln(w, \"ok\")\n})\nlog.Fatal(http.ListenAndServe(\":@@8080\", mux))";
    "handler" => "http handler", "HandlerFunc",
        "func @@(w http.ResponseWriter, r *http.Request) {\n\tw.Header().Set(\"Content-Type\", \"application/json\")\n\tjson.NewEncoder(w).Encode(map[string]string{\"status\": \"ok\"})\n}";
    "goro" => "goroutine", "go func with WaitGroup",
        "var wg sync.WaitGroup\nwg.Add(1)\ngo func() {\n\tdefer wg.Done()\n\t@@\n}()\nwg.Wait()";
    "chan" => "channel", "Buffered channel + select",
        "ch := make(chan @@int, 1)\nch <- 1\nv := <-ch";
    "json" => "struct json tag", "Struct with tags + marshal",
        "type User struct {\n\tName string `json:\"name\"`\n\tAge  int    `json:\"age\"`\n}\n\nb, err := json.Marshal(@@&User{Name: \"ada\"})";
    "test" => "test func", "table-driven test hint",
        "func Test@@(t *testing.T) {\n\tgot := Want()\n\tif got != want {\n\t\tt.Errorf(\"got %v want %v\", got, want)\n\t}\n}";
    "map" => "map make", "map[string]...",
        "m := make(map[string]@@int)\nm[\"k\"] = 1";
    "defer" => "defer recover", "panic recovery",
        "defer func() {\n\tif r := recover(); r != nil {\n\t\tlog.Println(\"recovered:\", r)\n\t}\n}()@@";
    "file" => "read file", "os.ReadFile + error",
        "data, err := os.ReadFile(\"@@\")\nif err != nil {\n\tlog.Fatal(err)\n}\nfmt.Println(string(data))"
};

// ---------------------------------------------------------------- Rust
const RS: &[Snip] = snips! {
    "fn" => "fn", "Function",
        "fn @@() {\n    \n}";
    "main" => "fn main", "Program entry",
        "fn main() {\n    @@\n}";
    "struct" => "struct", "Struct with derive",
        "#[derive(Debug, Clone)]\nstruct @@ {\n    name: String,\n}";
    "enum" => "enum", "Enum with derive",
        "#[derive(Debug)]\nenum @@ {\n    A,\n    B,\n}";
    "impl" => "impl block", "Implementation block",
        "impl @@ {\n    pub fn new() -> Self {\n        Self {  }\n    }\n}";
    "match" => "match", "Pattern match",
        "match @@ {\n    Some(v) => v,\n    None => Default::default(),\n}";
    "test" => "#[test]", "Unit test",
        "#[test]\nfn test_@@() {\n    assert!(true);\n}";
    "iter" => "iterator chain", "map + filter + collect",
        "let out: Vec<_> = items.iter()\n    .filter(|x| x.is_active())\n    .map(|x| x.clone())\n    .collect();@@";
    "result" => "Result fn", "Fallible function",
        "fn @@() -> Result<(), Box<dyn std::error::Error>> {\n    Ok(())\n}";
    "read" => "read file", "fs::read_to_string",
        "let text = std::fs::read_to_string(\"@@\")?;";
    "thread" => "spawn thread", "std::thread::spawn",
        "let handle = std::thread::spawn(|| {\n    @@\n});\nhandle.join().unwrap();";
    "print" => "println!", "Formatted print",
        "println!(\"@@{:?}\", value);";
    "tokio" => "tokio main", "Async main",
        "#[tokio::main]\nasync fn main() {\n    @@\n}";
    "mod" => "module", "pub mod + use",
        "pub mod @@ {\n    pub fn run() {\n        \n    }\n}"
};

// ---------------------------------------------------------------- COBOL
const COB: &[Snip] = snips! {
    "id" => "IDENTIFICATION DIVISION", "Program identification",
        "IDENTIFICATION DIVISION.\nPROGRAM-ID. @@.\nAUTHOR. REA-STUDIO.";
    "env" => "ENVIRONMENT DIVISION", "File + config section",
        "ENVIRONMENT DIVISION.\nCONFIGURATION SECTION.\nSPECIAL-NAMES.\n    DECIMAL-POINT IS COMMA.@@";
    "data" => "WORKING-STORAGE", "Data division variables",
        "DATA DIVISION.\nWORKING-STORAGE SECTION.\n01 WS-NAME      PIC X(20).\n01 WS-AMOUNT    PIC 9(5)V99 VALUE ZERO.\n@@";
    "proc" => "PROCEDURE DIVISION", "Main procedure skeleton",
        "PROCEDURE DIVISION.\nMAIN-PARA.\n    DISPLAY \"@@HELLO\".\n    STOP RUN.";
    "display" => "DISPLAY", "Show output",
        "DISPLAY \"@@\" UPON CONSOLE.";
    "move" => "MOVE", "Assignment",
        "MOVE @@ TO WS-TARGET.";
    "compute" => "COMPUTE", "Arithmetic statement",
        "COMPUTE WS-RESULT = @@ WS-A + WS-B.";
    "if" => "IF / ELSE", "Conditional",
        "IF @@ WS-A > WS-B\n    DISPLAY \"GREATER\"\nELSE\n    DISPLAY \"LESS\"\nEND-IF.";
    "perform" => "PERFORM UNTIL", "Loop",
        "PERFORM UNTIL WS-IDX > 10\n    ADD 1 TO WS-IDX@@\nEND-PERFORM.";
    "performpara" => "PERFORM paragraph", "Call a paragraph",
        "PERFORM @@ PROCESS-PARA.";
    "read" => "READ file", "Sequential file read",
        "READ WS-FILE\n    AT END MOVE \"Y\" TO WS-EOF\nEND-READ.@@";
    "write" => "WRITE record", "Write to file",
        "WRITE WS-RECORD@@.";
    "accept" => "ACCEPT", "Read from stdin/date",
        "ACCEPT WS-INPUT@@ FROM CONSOLE.";
    "string" => "STRING statement", "Concatenate",
        "STRING WS-A DELIMITED BY SPACE\n       \" \" WS-B\n       INTO WS-RESULT@@\nEND-STRING."
};

// ---------------------------------------------------------------- PHP
const PHP: &[Snip] = snips! {
    "open" => "<?php", "Open tag",
        "<?php\n@@";
    "fn" => "function", "Function",
        "function @@() {\n    \n}";
    "class" => "class", "Class with constructor",
        "class @@ {\n    public function __construct(\n        private string $name,\n    ) {\n    }\n}";
    "echo" => "echo", "Output",
        "echo @@;";
    "fore" => "foreach", "Loop over array",
        "foreach ($items as $item) {\n    @@\n}";
    "arr" => "array", "Indexed + assoc array",
        "$data = [\n    'name' => '@@',\n    'value' => 1,\n];";
    "try" => "try / catch", "Exception handling",
        "try {\n    @@\n} catch (Exception $e) {\n    error_log($e->getMessage());\n}";
    "pdo" => "PDO connect", "Database connection",
        "$pdo = new PDO('mysql:host=localhost;dbname=@@app', $user, $pass, [\n    PDO::ATTR_ERRMODE => PDO::ERRMODE_EXCEPTION,\n]);";
    "json" => "json response", "JSON API response",
        "header('Content-Type: application/json');\necho json_encode([@@'status' => 'ok']);";
    "post" => "POST superglobal", "Read form input safely",
        "$value = filter_input(INPUT_POST, '@@value', FILTER_SANITIZE_SPECIAL_CHARS);";
    "funcn" => "nullable fn", "Arrow function",
        "$double = fn($x) => $x * @@2;";
    "interface" => "interface", "Interface",
        "interface @@ {\n    public function handle(): void;\n}";
    "trait" => "trait", "Trait",
        "trait @@ {\n    public function boot(): void {\n        \n    }\n}";
    "test" => "PHPUnit test", "Test case",
        "public function test_@@(): void {\n    $this->assertTrue(true);\n}"
};

// ---------------------------------------------------------------- Ruby
const RB: &[Snip] = snips! {
    "def" => "def method", "Method definition",
        "def @@\n  \nend";
    "class" => "class", "Class with initialize",
        "class @@\n  def initialize\n    \n  end\nend";
    "module" => "module", "Module namespace",
        "module @@\n  VERSION = \"1.0.0\"\nend";
    "each" => "each block", "Iterate collection",
        "items.each do |item|\n  @@\nend";
    "map" => "map block", "Transform collection",
        "items.map { |x| @@ }";
    "if" => "if / else", "Conditional",
        "if @@\n  \nelse\n  \nend";
    "unless" => "unless", "Negative conditional",
        "unless @@\n  \nend";
    "begin" => "begin / rescue", "Exception handling",
        "begin\n  @@\nrescue StandardError => e\n  puts e.message\nend";
    "attr" => "attr_accessor", "Attribute accessors",
        "attr_accessor :@@name";
    "req" => "require", "Require gem/file",
        "require '@@json'";
    "puts" => "puts", "Print line",
        "puts @@";
    "lambda" => "lambda", "Lambda literal",
        "double = ->(x) { x * @@2 }";
    "struct" => "Struct", "Lightweight data class",
        "Point = Struct.new(:x, :y) do\n  def dist\n    Math.sqrt(x**2 + y**2)@@\n  end\nend";
    "rails" => "Rails controller", "REST controller skeleton",
        "class @@Controller < ApplicationController\n  before_action :set_item, only: %i[show update destroy]\n\n  def index\n    render json: Item.all\n  end\nend"
};

// ---------------------------------------------------------------- Swift
const SWIFT: &[Snip] = snips! {
    "func" => "func", "Function",
        "func @@() {\n    \n}";
    "view" => "SwiftUI View", "View struct with body",
        "struct @@ContentView: View {\n    var body: some View {\n        Text(\"Hello, world!\")\n            .padding()\n    }\n}";
    "class" => "class", "Class definition",
        "class @@ {\n    init() {\n        \n    }\n}";
    "struct" => "struct", "Struct + Codable",
        "struct @@: Codable {\n    let name: String\n}";
    "enum" => "enum", "Enum with cases",
        "enum @@ {\n    case loading\n    case loaded\n}";
    "guard" => "guard let", "Early exit binding",
        "guard let value = @@optional else { return }";
    "async" => "async fn", "Async function with await",
        "func load() async throws -> Data {\n    let (data, _) = try await URLSession.shared.data(from: @@url)\n    return data\n}";
    "closure" => "closure", "Trailing closure",
        "items.map { item in\n    @@item\n}";
    "ext" => "extension", "Extension",
        "extension @@String {\n    func trimmed() -> String {\n        trimmingCharacters(in: .whitespaces)\n    }\n}";
    "observed" => "@ObservableObject", "Stateful view model",
        "class @@Model: ObservableObject {\n    @Published var value = 0\n}";
    "iflet" => "if let", "Optional binding",
        "if let value = @@optional {\n    print(value)\n}";
    "protocol" => "protocol", "Protocol definition",
        "protocol @@ {\n    func run()\n}";
    "for" => "for loop", "Range loop",
        "for i in 0..<@@10 {\n    print(i)\n}";
    "json" => "JSON decode", "Codable decode",
        "let decoded = try JSONDecoder().decode(@@Item.self, from: data)"
};

// ---------------------------------------------------------------- Perl
const PL: &[Snip] = snips! {
    "open" => "script header", "shebang + strict/warnings",
        "#!/usr/bin/perl\nuse strict;\nuse warnings;\nuse utf8;\n\n@@";
    "sub" => "sub", "Subroutine",
        "sub @@ {\n    my ($self) = @_;\n    return;\n}";
    "fore" => "foreach", "Loop over list",
        "foreach my $item (@items) {\n    @@\n}";
    "while" => "while loop", "Read lines from file",
        "open my $fh, '<', '@@file.txt' or die \"$!\";\nwhile (my $line = <$fh>) {\n    chomp $line;\n    print \"$line\\n\";\n}\nclose $fh;";
    "if" => "if / else", "Conditional",
        "if (@@$a > $b) {\n    \n} else {\n    \n}";
    "regex" => "regex match", "Pattern match + capture",
        "if ($str =~ /^(\\w+)@(\\w+)$/) {\n    my ($user, $host) = ($1, $2);@@\n}";
    "subrx" => "regex substitute", "s/// replacement",
        "$str =~ s/@@old/new/g;";
    "hash" => "hash", "Hash literal",
        "my %data = (\n    name => '@@ada',\n    age  => 36,\n);";
    "array" => "array", "Array literal",
        "my @items = (@@1, 2, 3);";
    "http" => "LWP GET", "HTTP request with LWP",
        "use LWP::UserAgent;\nmy $ua  = LWP::UserAgent->new(timeout => 10);\nmy $res = $ua->get('@@http://example.com');\nprint $res->decoded_content if $res->is_success;";
    "json" => "JSON encode", "JSON module usage",
        "use JSON;\nmy $json = encode_json(\\@@%data);";
    "print" => "print", "Formatted print",
        "printf \"@@%s: %d\\n\", $name, $value;";
    "die" => "die / warn", "Error handling",
        "open my $fh, '<', $path or die \"Cannot open $path: @@$!\";";
    "module" => "package", "Perl module skeleton",
        "package @@My::Module;\nuse strict;\nuse warnings;\nuse Exporter 'import';\nour @EXPORT_OK = qw(run);\n\nsub run {\n    \n}\n\n1;"
};

// ---------------------------------------------------------------- C#
const CS: &[Snip] = snips! {
    "class" => "class", "Public class",
        "public class @@\n{\n    \n}";
    "main" => "Main method", "Program entry point",
        "static void Main(string[] args)\n{\n    @@\n}";
    "prop" => "property", "Auto-property",
        "public string @@Name { get; set; }";
    "method" => "async method", "Async Task method",
        "public async Task @@DoWorkAsync()\n{\n    await Task.Delay(100);\n}";
    "linq" => "LINQ query", "Where + Select",
        "var result = items\n    .Where(x => x.IsActive)\n    .Select(x => x.Name)\n    .ToList();@@";
    "foreach" => "foreach", "Loop",
        "foreach (var item in @@items)\n{\n    \n}";
    "tryc" => "try / catch", "Exception handling",
        "try\n{\n    @@\n}\ncatch (Exception ex)\n{\n    Console.WriteLine(ex.Message);\n}";
    "ctor" => "constructor", "Primary-style constructor",
        "public @@(string name)\n{\n    Name = name;\n}";
    "interface" => "interface", "Interface",
        "public interface I@@\n{\n    void Run();\n}";
    "record" => "record", "Immutable record",
        "public record @@(string Name, int Age);";
    "httpclient" => "HttpClient GET", "API call",
        "using var http = new HttpClient();\nvar json = await http.GetStringAsync(\"@@https://api.example.com\");";
    "webapi" => "Minimal API", "ASP.NET Core endpoint",
        "var app = WebApplication.Create(args);\n\napp.MapGet(\"/\", () => Results.Json(new { status = \"ok\" }));\n\napp.Run();@@";
    "dict" => "Dictionary", "Dictionary init",
        "var map = new Dictionary<string, int>\n{\n    [\"@@a\"] = 1,\n};";
    "test" => "xUnit test", "Fact test",
        "[Fact]\npublic void @@Test_Sum()\n{\n    Assert.Equal(2, 1 + 1);\n}"
};

// ---------------------------------------------------------------- Kotlin
const KT: &[Snip] = snips! {
    "fun" => "fun", "Function",
        "fun @@() {\n    \n}";
    "main" => "main", "Program entry",
        "fun main() {\n    @@\n}";
    "class" => "class", "Class with primary constructor",
        "class @@(val name: String) {\n    \n}";
    "data" => "data class", "Immutable data class",
        "data class @@(val name: String, val age: Int)";
    "object" => "object", "Singleton object",
        "object @@ {\n    const val TAG = \"app\"\n}";
    "when" => "when", "Switch-like expression",
        "when (@@value) {\n    1 -> println(\"one\")\n    else -> println(\"other\")\n}";
    "null" => "safe call", "?. + ?: elvis",
        "val len = @@value?.length ?: 0";
    "lambda" => "lambda", "Collection lambda",
        "val names = items.map { it.name }@@";
    "ext" => "extension fun", "Extension function",
        "fun String.@@shout(): String = uppercase() + \"!\"";
    "corr" => "coroutine", "runBlocking + async",
        "runBlocking {\n    val result = async {\n        @@\n    }.await\n}";
    "comp" => "companion object", "Static members",
        "companion object {\n    private const val TAG = \"@@app\"\n}";
    "sealed" => "sealed class", "UI state pattern",
        "sealed class @@State {\n    object Loading : State()\n    data class Data(val items: List<String>) : State()\n}";
    "interface" => "interface", "Interface",
        "interface @@ {\n    fun run()\n}";
    "test" => "kotlin.test", "Test function",
        "@Test\nfun `@@sum works`() {\n    assertEquals(2, 1 + 1)\n}"
};

// ---------------------------------------------------------------- F#
const FS: &[Snip] = snips! {
    "let" => "let binding", "Immutable value",
        "let @@answer = 42";
    "fn" => "function", "let-bound function",
        "let @@add a b = a + b";
    "module" => "module", "Module with doc",
        "module @@Math =\n    let square x = x * x";
    "record" => "record type", "Immutable record",
        "type @@Person = { Name: string; Age: int }";
    "union" => "discriminated union", "DU definition",
        "type @@Shape =\n    | Circle of float\n    | Rect of float * float";
    "match" => "match", "Pattern matching",
        "match @@value with\n    | Some x -> x\n    | None -> 0";
    "pipe" => "pipeline", "|> forward pipe",
        "items\n|> List.filter (fun x -> x > @@0)\n|> List.sum";
    "async" => "async workflow", "async { } block",
        "let job = async {\n    do! Async.Sleep @@100\n    return 42\n}";
    "class" => "class", "Class with member",
        "type @@() =\n    member _.Run() =\n        printfn \"running\"";
    "main" => "main", "Program entry",
        "[<EntryPoint>]\nlet main argv =\n    @@printfn \"hello\"\n    0";
    "list" => "list", "List literal",
        "let nums = [ @@1; 2; 3 ]";
    "try" => "try / with", "Exception handling",
        "try\n    @@failwith \"boom\"\nwith\n| :? System.Exception as ex -> printfn \"%s\" ex.Message";
    "option" => "Option helpers", "Option.map/defaultValue",
        "value |> Option.map (fun x -> x * @@2) |> Option.defaultValue 0";
    "seq" => "seq expression", "Lazy sequence",
        "let squares = seq {\n    for i in 1..@@10 do\n        yield i * i\n}"
};

// ---------------------------------------------------------------- registry

/// ext -> language table
pub fn for_ext(ext: &str) -> &'static [Snip] {
    match ext {
        "py" | "pyw" => PY,
        "html" | "htm" | "xhtml" | "vue" | "svelte" => HTML,
        "css" | "scss" | "sass" | "less" => CSS,
        "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx" => JS,
        "go" => GO,
        "rs" => RS,
        "cob" | "cobol" | "cbl" => COB,
        "php" => PHP,
        "rb" => RB,
        "swift" => SWIFT,
        "pl" | "pm" => PL,
        "cs" => CS,
        "kt" | "kts" => KT,
        "fs" | "fsi" | "fsx" => FS,
        _ => &[],
    }
}

/// Language display name for the popup header / status messages.
pub fn lang_name(ext: &str) -> &'static str {
    match ext {
        "py" | "pyw" => "Python",
        "html" | "htm" | "xhtml" => "HTML",
        "css" | "scss" | "sass" | "less" => "CSS",
        "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx" => "JavaScript",
        "go" => "Go",
        "rs" => "Rust",
        "cob" | "cobol" | "cbl" => "COBOL",
        "php" => "PHP",
        "rb" => "Ruby",
        "swift" => "Swift",
        "pl" | "pm" => "Perl",
        "cs" => "C#",
        "kt" | "kts" => "Kotlin",
        "fs" | "fsi" | "fsx" => "F#",
        _ => "",
    }
}

/// Recommendations for `prefix` (word under caret), best-first.
pub fn filter_snips(ext: &str, prefix: &str, cap: usize) -> Vec<Snip> {
    let table = for_ext(ext);
    if table.is_empty() {
        return Vec::new();
    }
    let pl = prefix.to_lowercase();
    let mut out: Vec<Snip> = table
        .iter()
        .filter(|s| pl.is_empty() || s.trig.starts_with(&pl) || s.label.to_lowercase().starts_with(&pl))
        .take(cap)
        .cloned()
        .collect();
    if !pl.is_empty() {
        // exact trigger first
        out.sort_by_key(|s| if s.trig == pl { 0 } else { 1 });
    }
    out
}

/// Expand a template: returns (text, caret_char_pos_in_text).
pub fn expand(code: &str) -> (String, usize) {
    match code.find("@@") {
        Some(i) => (format!("{}{}", &code[..i], &code[i + 2..]), i),
        None => (code.to_string(), code.len()),
    }
}
