@echo off
setlocal EnableExtensions DisableDelayedExpansion
set "CARGO_PROFILE_DEV_CODEGEN_UNITS=1"
cd /d H:\Projects\rad-lang
call devtools\rustc-memory\capture.cmd D:\Temp\rad-rustc-profile\rad-vm-cgu1.etl "cargo.exe +stable-x86_64-pc-windows-gnu build -p rad-vm --no-default-features -j 1"
exit /b %ERRORLEVEL%
