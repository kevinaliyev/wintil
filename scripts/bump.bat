@echo off
setlocal
cd /d "%~dp0\.."
python scripts\bump_version.py %*
endlocal
