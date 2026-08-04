$ErrorActionPreference = "Stop"

$Repo = Resolve-Path (Join-Path $PSScriptRoot "..\..\..")
$Rad = Join-Path $Repo "target\release\rad.exe"
$Out = Join-Path $PSScriptRoot "out"
$Certificate = Join-Path $Out "certificate.json"
$Verification = Join-Path $Out "verification.json"
$Trace = Join-Path $Out "run.radr"
$TailCertificate = Join-Path $Out "natural-tail-certificate.json"
$TailVerification = Join-Path $Out "natural-tail-verification.json"
$TailTrace = Join-Path $Out "natural-tail.radr"
$SupportCertificate = Join-Path $Out "support-pressure-certificate.json"
$SupportVerification = Join-Path $Out "support-pressure-independent-verification.json"
$SupportTrace = Join-Path $Out "support-pressure.radr"
$FrontierReport = Join-Path $Out "frontier-1024.json"
$FrontierVerification = Join-Path $Out "frontier-1024-verification.json"
$FrontierTrace = Join-Path $Out "frontier-1024.radr"

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

    & $Rad "projects/dogfood/collatz-lab/natural_tail.rad" "--experimental-laws" "--record" $TailTrace
    if ($LASTEXITCODE -ne 0) { throw "Collatz natural-tail RAD run failed" }

    python "projects/dogfood/collatz-lab/verify_natural_tail.py" $TailCertificate "--report" $TailVerification
    if ($LASTEXITCODE -ne 0) { throw "Independent natural-tail verification failed" }

    & $Rad "replay" $TailTrace
    if ($LASTEXITCODE -ne 0) { throw "Collatz natural-tail replay failed" }

    & $Rad "projects/dogfood/collatz-lab/support_pressure.rad" "--experimental-laws" "--record" $SupportTrace
    if ($LASTEXITCODE -ne 0) { throw "Collatz support-pressure RAD run failed" }

    python "projects/dogfood/collatz-lab/verify_support_pressure.py" $SupportCertificate "--report" $SupportVerification
    if ($LASTEXITCODE -ne 0) { throw "Independent support-pressure verification failed" }

    & $Rad "replay" $SupportTrace
    if ($LASTEXITCODE -ne 0) { throw "Collatz support-pressure replay failed" }

    & $Rad "projects/dogfood/collatz-lab/frontier_probe.rad" "--experimental-laws" "--record" $FrontierTrace "--" "1024" "14" "256" | Set-Content -Encoding UTF8 $FrontierReport
    if ($LASTEXITCODE -ne 0) { throw "Collatz frontier synthesis failed" }

    python "projects/dogfood/collatz-lab/verify_frontier.py" $FrontierReport "--output" $FrontierVerification
    if ($LASTEXITCODE -ne 0) { throw "Independent frontier witness verification failed" }

    & $Rad "replay" $FrontierTrace
    if ($LASTEXITCODE -ne 0) { throw "Collatz frontier replay failed" }
} finally {
    Pop-Location
}
