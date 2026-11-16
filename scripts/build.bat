@echo off
setlocal
cd /d "%~dp0\.."
echo [*] Running Cargo tests...
cargo test || exit /b 1

echo [*] Stopping running Wintil instances...
schtasks.exe /End /TN Wintil >nul 2>&1

echo [*] Compiling release binary...
cargo build --release || exit /b 1

echo [*] Relaunching Wintil service...
schtasks.exe /Run /TN Wintil >nul 2>&1
echo [SUCCESS] Wintil compiled and active!
endlocal
