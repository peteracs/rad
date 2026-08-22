$ErrorActionPreference = "Stop"

if (-not ($IsWindows -or $env:OS -eq "Windows_NT")) {
    throw "This setup script is only for Windows hosts."
}

$toolchain = "stable-x86_64-pc-windows-gnu"
$installed = rustup toolchain list
if ($LASTEXITCODE -ne 0) {
    throw "rustup toolchain list failed with exit code $LASTEXITCODE"
}

if (-not ($installed -match "^$([regex]::Escape($toolchain))(?:\s|$)")) {
    rustup toolchain install $toolchain
    if ($LASTEXITCODE -ne 0) {
        throw "rustup toolchain install failed with exit code $LASTEXITCODE"
    }
}

# A rustup directory override is inherited by every Cargo invocation below this
# checkout, while Linux and macOS contributors retain their native toolchains.
rustup override set $toolchain
if ($LASTEXITCODE -ne 0) {
    throw "rustup override set failed with exit code $LASTEXITCODE"
}

$active = rustup show active-toolchain
if ($LASTEXITCODE -ne 0 -or $active -notmatch "^$([regex]::Escape($toolchain))\s") {
    throw "Expected $toolchain, but rustup reported: $active"
}

$gcc = Get-Command gcc.exe -CommandType Application -ErrorAction SilentlyContinue
if (-not $gcc) {
    throw "The GNU Rust host requires a MinGW-w64 gcc.exe on PATH. Install MSYS2 MinGW x64 and rerun this script."
}

$gccTarget = & $gcc.Source -dumpmachine
if ($LASTEXITCODE -ne 0 -or $gccTarget -ne "x86_64-w64-mingw32") {
    throw "Expected x86_64-w64-mingw32 GCC, but '$($gcc.Source)' reported '$gccTarget'."
}

Write-Host "RAD Windows Rust toolchain: $active"
Write-Host "RAD Windows linker driver: $($gcc.Source) ($gccTarget)"
