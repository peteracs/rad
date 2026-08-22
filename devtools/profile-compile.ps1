# Compile-time/memory evidence for one crate: rustc self-profile + monomorphization stats.
#
# Uses nightly-x86_64-pc-windows-gnu explicitly. Plain `+nightly` resolves to the MSVC
# nightly, which cannot link here: D:\msys64\usr\bin\link.exe (POSIX coreutils `link`)
# shadows the MSVC linker on PATH, and the BuildTools VC bin directory is not on PATH at
# all. The GNU toolchain also matches the ABI every other measurement was taken with.

param(
    [string] $Package = "rad-vm",
    [string] $OutRoot = "D:\Temp\rad-nightly-profile"
)

$ErrorActionPreference = "Stop"

# PowerShell 5.1 wraps a native command's stderr in ErrorRecords, which "Stop" then turns
# into a terminating error even on exit code 0. cargo and summarize both write progress to
# stderr, so their exit codes are checked explicitly instead.
function Invoke-Native {
    param([scriptblock] $Command)

    $previous = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try { & $Command } finally { $ErrorActionPreference = $previous }
}

$Toolchain = "nightly-x86_64-pc-windows-gnu"
$SelfProfile = Join-Path $OutRoot "self-profile"
$MonoStats = Join-Path $OutRoot "mono-stats"
$TargetDir = Join-Path $OutRoot "target"

# Start from an empty profile directory and force $Package to rebuild. Without this, an
# up-to-date crate produces no new data and the summary below would silently describe an
# earlier run.
Remove-Item -Recurse -Force -LiteralPath $SelfProfile, $MonoStats -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $SelfProfile, $MonoStats | Out-Null
Invoke-Native { cargo "+$Toolchain" clean -p $Package --target-dir $TargetDir }
if ($LASTEXITCODE -ne 0) {
    throw "cargo clean failed with exit code $LASTEXITCODE"
}

# A dedicated target directory keeps the nightly artifacts from evicting the stable build cache.
Invoke-Native { cargo "+$Toolchain" rustc -p $Package --lib --no-default-features --target-dir $TargetDir -- `
    "-Zself-profile=$SelfProfile" `
    "-Zdump-mono-stats=$MonoStats" `
    -Zdump-mono-stats-format=json }
if ($LASTEXITCODE -ne 0) {
    throw "compile failed with exit code $LASTEXITCODE"
}

$Profdata = Get-ChildItem -LiteralPath $SelfProfile -Filter "*.mm_profdata" |
    Sort-Object LastWriteTime |
    Select-Object -Last 1
if (-not $Profdata) {
    throw "no .mm_profdata was produced in $SelfProfile"
}

# `summarize` ships with measureme: cargo install --git https://github.com/rust-lang/measureme summarize
Invoke-Native { summarize summarize $Profdata.FullName }
