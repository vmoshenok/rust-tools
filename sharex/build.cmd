@echo off
rem Builds target\...\release\rustshot.exe.
rem Uses the MSVC toolchain if Visual Studio Build Tools (link.exe) are available,
rem otherwise the GNU toolchain (rustup toolchain install stable-x86_64-pc-windows-gnu).
where link.exe >nul 2>nul
if %errorlevel%==0 (
    cargo build --release
) else (
    cargo +stable-x86_64-pc-windows-gnu build --release
)
