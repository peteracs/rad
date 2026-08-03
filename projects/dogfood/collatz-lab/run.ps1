$ErrorActionPreference = "Stop"

$Repo = Resolve-Path (Join-Path $PSScriptRoot "..\..\..")
$Rad = Join-Path $Repo "target\release\rad.exe"
$Out = Join-Path $PSScriptRoot "out"
$Certificate = Join-Path $Out "certificate.json"
$Verification = Join-Path $Out "verification.json"
$Trace = Join-Path $Out "run.radr"

if (-not (Test-Path -LiteralPath $Rad)) {
    throw "Build RAD first: cargo build --release -p rad-vm"
}

New-Item -ItemType Directory -Force -Path $Out | Out-Null
Push-Location $Repo
try {
    & "projects/dogfood/native-math-kernels/build.ps1"
    if ($LASTEXITCODE -ne 0) { throw "Native math kernel build failed" }

    & $Rad "projects/dogfood/collatz-lab/main.rad" "--experimental-laws" "--record" $Trace
    if ($LASTEXITCODE -ne 0) { throw "Collatz RAD run failed" }

    python "projects/dogfood/collatz-lab/verify_certificate.py" $Certificate "--report" $Verification
    if ($LASTEXITCODE -ne 0) { throw "Independent certificate verification failed" }

    & $Rad "replay" $Trace
    if ($LASTEXITCODE -ne 0) { throw "Collatz replay failed" }
} finally {
    Pop-Location
}
