# REA Studio

A lightweight and fast code editor built with **Rust** and **egui**.

**Current version: 2.1.0**

## Download — v2.1.0

| Platform | Download |
|---|---|
| Windows Installer | [REAStudio-Setup-2.1.0.exe](https://github.com/smartlmgpt-create/rea-studio/releases/download/v2.1.0/REAStudio-Setup-2.1.0.exe) |
| Windows Portable (if available) | [REAStudio-2.1.0.exe](https://github.com/smartlmgpt-create/rea-studio/releases/download/v2.1.0/REAStudio-2.1.0.exe) |
| Linux Installer | [REAStudio-2.1.0-linux-setup.sh](https://github.com/smartlmgpt-create/rea-studio/releases/download/v2.1.0/REAStudio-2.1.0-linux-setup.sh) |
| macOS Installer | [REAStudio-2.1.0-macos-setup.sh](https://github.com/smartlmgpt-create/rea-studio/releases/download/v2.1.0/REAStudio-2.1.0-macos-setup.sh) |
| Source Code | [rea-studio-2.1.0-src.zip](https://github.com/smartlmgpt-create/rea-studio/releases/download/v2.1.0/rea-studio-2.1.0-src.zip) |

**[View all releases](https://github.com/smartlmgpt-create/rea-studio/releases)**

## In-App Update Checker

REA Studio checks the following manifest for available updates:

`https://raw.githubusercontent.com/smartlmgpt-create/rea-studio/main/latest.json`

To check for updates manually, open:

**Help → Check for Updates**

The manifest URL is stored in the application's `settings.json` file and can also be changed through Settings.

## Features in Version 2.1.0

- **27 text tools:** Sort lines, remove duplicates, change letter case, Base64, URL and JSON conversion, MD5/SHA-1/SHA-256 hashing, EOL/BOM handling, and timestamp insertion.
- **File Comparison:** Side-by-side comparison with navigation between differences using F8.
- **TODO Scanner:** Scan the entire project folder for TODO, FIXME, XXX, and HACK comments.
- **Document Statistics:** View line, word, character, and selection counts, along with LF/CRLF indicators.
- **Keyboard Macros and Column Selection:** Features introduced in version 2.0.
- **55+ Language Highlighting:** Syntax highlighting for more than 55 languages.
- **Integrated Terminal**
- **Language Server Protocol (LSP) Support**
- **Plugin Support**
- **Dark and Light Themes**
- **Bookmarks and Function Outline**
- **Zoom Controls:** Use Ctrl + Mouse Wheel.
- **Session Recovery**

## Building from Source

Make sure Rust and Cargo are installed, then run:

```bash
cargo build --release
```

For detailed build instructions, see:

- [BUILDING.md](BUILDING.md)
- [CHANGELOG-2.1.0.md](CHANGELOG-2.1.0.md)

## License

REA Studio is distributed under the MIT License. See [LICENSE](LICENSE) for details.

## Repository

[https://github.com/smartlmgpt-create/rea-studio](https://github.com/smartlmgpt-create/rea-studio)
