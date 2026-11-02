# Building REA Studio on Windows

## Toolchain
Cross-compile from Linux with llvm-mingw (https://github.com/mstorsjo/llvm-mingw):

    rustup target add x86_64-pc-windows-gnullvm
    PATH=/path/to/llvm-mingw/bin:$PATH
    cargo build --release --target x86_64-pc-windows-gnullvm

## IMPORTANT: avoid libunwind.dll runtime dependency
By default the linker resolves `-lunwind` to the **dynamic** import library
(`libunwind.dll.a`), so the built exe requires `libunwind.dll` at runtime and
Windows shows:  "The code execution cannot proceed because libunwind.dll was
not found."

Fix (pick one):

1. Recommended — hide the import library in your llvm-mingw toolchain so only
   the static archive is ever used:

       mv <llvm-mingw>/x86_64-w64-mingw32/lib/libunwind.dll.a \
          <llvm-mingw>/x86_64-w64-mingw32/lib/libunwind.dll.a.disabled

2. Or link fully static:

       RUSTFLAGS="-C link-arg=-static" cargo build --release \
           --target x86_64-pc-windows-gnullvm

Verify afterwards — the exe must NOT list libunwind.dll:

    objdump -p target/x86_64-pc-windows-gnullvm/release/rea-studio.exe | grep "DLL Name"

All remaining imports (kernel32, user32, gdi32, opengl32, api-ms-win-crt-*,
ntdll, ...) are standard Windows 10/11 system components; nothing else to ship.

## v1.7.0 notes
New target-specific dependencies (Windows only, no OpenSSL needed):

    [target.'cfg(windows)'.dependencies]
    windows-sys = "0.59"   # ConPTY pseudo console (terminal redesign)
    native-tls  = "0.2"    # SChannel TLS for the Live Server HTTPS mode

On Linux host checks (`cargo check`) these are skipped automatically; the
exe links only system DLLs (kernel32, secur32, crypt32, ws2_32, …).

## NSIS installer
    # Linux: extract the nsis + nsis-common .deb packages, then
    NSISDIR=<extracted>/usr/share/nsis <extracted>/usr/bin/makensis installer.nsi
    # → REAStudio-Setup-1.7.0.exe
