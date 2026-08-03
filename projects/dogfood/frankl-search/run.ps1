param(
    [string]$Rad = "target/release/rad.exe",
    [switch]$FullStructuralVerification
)

$ErrorActionPreference = "Stop"
$project = Split-Path -Parent $MyInvocation.MyCommand.Path
$repo = Resolve-Path (Join-Path $project "../../..")
$certificate = Join-Path $project "out/latest.json"
$report = Join-Path $project "out/latest.verified.json"
$trace = Join-Path $project "out/latest.radr"
$deletionCertificate = Join-Path $project "out/deletion-2048.json"

Push-Location $repo
try {
    & "projects/dogfood/native-math-kernels/build.ps1"
    if ($LASTEXITCODE -ne 0) { throw "Native math kernel build failed" }

    & $Rad "projects/dogfood/frankl-search/exhaustive_n4.rad"
    if ($LASTEXITCODE -ne 0) { throw "N=4 exhaustive audit failed" }

    & $Rad "projects/dogfood/frankl-search/search.rad" "--experimental-laws" "--record" $trace
    if ($LASTEXITCODE -ne 0) { throw "N=13 RAD search failed" }

    & $Rad "replay" $trace
    if ($LASTEXITCODE -ne 0) { throw "N=13 record/replay verification failed" }

    python "projects/dogfood/frankl-search/verify_certificate.py" $certificate --report $report
    if ($LASTEXITCODE -ne 0) { throw "independent certificate verification failed" }

    python -O "projects/dogfood/frankl-search/verify_certificate.py" $certificate | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "optimized-Python verifier failed" }

    & $Rad "projects/dogfood/frankl-search/cyclic_universes.rad" "--experimental-laws"
    if ($LASTEXITCODE -ne 0) { throw "exact cyclic-universe study failed" }

    $structural = "projects/dogfood/frankl-search/out/cyclic-universes.json"
    if ($FullStructuralVerification) {
        python "projects/dogfood/frankl-search/verify_structure.py" $structural
    }
    else {
        python "projects/dogfood/frankl-search/verify_structure.py" $structural --quick
    }
    if ($LASTEXITCODE -ne 0) { throw "independent structural verification failed" }

    & $Rad "projects/dogfood/frankl-search/deletion_search.rad" `
        "--experimental-laws" "--" `
        1 1 1 6144 8192 "-" 2048 $deletionCertificate 0 "max_min" "ordered"
    if ($LASTEXITCODE -ne 0) { throw "legal-deletion search failed" }

    python "projects/dogfood/frankl-search/verify_deletion.py" $deletionCertificate
    if ($LASTEXITCODE -ne 0) { throw "independent deletion verification failed" }

    & $Rad "projects/dogfood/frankl-search/boundary_analysis.rad" `
        "--experimental-laws" "--" $deletionCertificate
    if ($LASTEXITCODE -ne 0) { throw "causal boundary analysis failed" }
}
finally {
    Pop-Location
}
