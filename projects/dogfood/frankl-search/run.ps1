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
$balancedCertificate = Join-Path $project "out/deletion-balanced-51.json"
$orbitCertificate = Join-Path $project "out/orbit-search-6-causal.json"
$orbitReport = Join-Path $project "out/orbit-search-6-causal.verified.json"
$regularCertificate = Join-Path $project "certificates/regular-maxsat-proper-n13.json"
$regularReport = Join-Path $project "out/regular-maxsat-proper-n13.verified.json"
$regularTrace = Join-Path $project "out/regular-proof.radr"
$symmetryCertificate = Join-Path $project "certificates/symmetry-exclusions-n13.json"
$symmetryReport = Join-Path $project "out/symmetry-exclusions-n13.verified.json"
$symmetryTrace = Join-Path $project "out/symmetry-proof.radr"
$layeringCertificate = Join-Path $project "certificates/cyclic-layering-n13.json"
$layeringReport = Join-Path $project "out/cyclic-layering-n13.verified.json"
$layeringTrace = Join-Path $project "out/layering-proof.radr"
$generatorFrontierCertificate = Join-Path $project "certificates/generator-frontier-n13.json"
$generatorFrontierTrace = Join-Path $project "out/generator-frontier.radr"
$generatorBoundTrace = Join-Path $project "out/generator-bound.radr"
$generatorEightFrontierCertificate = Join-Path $project "certificates/generator8-graph-frontier.json"
$generatorEightFrontierTrace = Join-Path $project "out/generator8-frontier.radr"
$projectedPartitionTrace = Join-Path $project "out/projected-partition-proof.radr"
$generatorEightQ13Certificate = Join-Path $project "certificates/generator8-q13-exclusions.json"
$generatorEightQ13Trace = Join-Path $project "out/generator8-q13-exclusions.radr"

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

    & $Rad "projects/dogfood/frankl-search/deletion_search.rad" `
        "--experimental-laws" "--" `
        1 8 32 8141 8192 "-" 51 $balancedCertificate 400 "max_min" "ordered" "delete"
    if ($LASTEXITCODE -ne 0) { throw "balanced 51-set search failed" }

    python "projects/dogfood/frankl-search/verify_deletion.py" $balancedCertificate
    if ($LASTEXITCODE -ne 0) { throw "balanced deletion verification failed" }

    python "projects/dogfood/frankl-search/analyze_deletion.py" $balancedCertificate | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "independent boundary analysis failed" }

    & $Rad "projects/dogfood/frankl-search/pair_pressure.rad" `
        "--experimental-laws" "--" $balancedCertificate
    if ($LASTEXITCODE -ne 0) { throw "causal pair-pressure analysis failed" }

    & $Rad "projects/dogfood/frankl-search/orbit_search.rad" `
        "--experimental-laws" "--" `
        24 8 8 32 6 $orbitCertificate
    if ($LASTEXITCODE -ne 0) { throw "regular orbit-world search failed" }

    python "projects/dogfood/frankl-search/verify_orbit_search.py" `
        $orbitCertificate --output $orbitReport
    if ($LASTEXITCODE -ne 0) { throw "independent orbit certificate verification failed" }

    python "projects/dogfood/frankl-search/verify_regular_proof.py" `
        $regularCertificate --output $regularReport
    if ($LASTEXITCODE -ne 0) { throw "regular-class structural certificate verification failed" }

    & $Rad "projects/dogfood/frankl-search/regular_proof.rad" `
        "--experimental-laws" "--record" $regularTrace
    if ($LASTEXITCODE -ne 0) { throw "causal regular-class proof audit failed" }

    & $Rad "replay" $regularTrace
    if ($LASTEXITCODE -ne 0) { throw "causal regular-class proof replay failed" }

    python "projects/dogfood/frankl-search/verify_symmetry_suite.py" `
        $symmetryCertificate --output $symmetryReport
    if ($LASTEXITCODE -ne 0) { throw "symmetry-suite structural verification failed" }

    & $Rad "projects/dogfood/frankl-search/symmetry_proof.rad" `
        "--experimental-laws" "--record" $symmetryTrace
    if ($LASTEXITCODE -ne 0) { throw "causal symmetry-suite proof audit failed" }

    & $Rad "replay" $symmetryTrace
    if ($LASTEXITCODE -ne 0) { throw "causal symmetry-suite proof replay failed" }

    python "projects/dogfood/frankl-search/layering_lemma.py" `
        $layeringCertificate --output $layeringReport
    if ($LASTEXITCODE -ne 0) { throw "coprime-cycle layering verification failed" }

    & $Rad "projects/dogfood/frankl-search/layering_proof.rad" `
        "--experimental-laws" "--record" $layeringTrace
    if ($LASTEXITCODE -ne 0) { throw "causal coprime-cycle layering audit failed" }

    & $Rad "replay" $layeringTrace
    if ($LASTEXITCODE -ne 0) { throw "causal coprime-cycle layering replay failed" }

    & $Rad "projects/dogfood/frankl-search/generator_bound_proof.rad" `
        "--experimental-laws" "--record" $generatorBoundTrace
    if ($LASTEXITCODE -ne 0) { throw "bounded-generator causal proof failed" }

    & $Rad "replay" $generatorBoundTrace
    if ($LASTEXITCODE -ne 0) { throw "bounded-generator proof replay failed" }

    & $Rad "projects/dogfood/frankl-search/generator_frontier.rad" `
        "--experimental-laws" "--record" $generatorFrontierTrace "--" $generatorFrontierCertificate
    if ($LASTEXITCODE -ne 0) { throw "seven-generator frontier scan failed" }

    & $Rad "replay" $generatorFrontierTrace
    if ($LASTEXITCODE -ne 0) { throw "seven-generator frontier replay failed" }

    python "projects/dogfood/frankl-search/verify_generator_frontier.py" `
        $generatorFrontierCertificate
    if ($LASTEXITCODE -ne 0) { throw "seven-generator frontier verification failed" }

    python "projects/dogfood/frankl-search/verify_generator8_frontier.py" `
        $generatorEightFrontierCertificate
    if ($LASTEXITCODE -ne 0) { throw "eight-generator graph frontier verification failed" }

    & $Rad "projects/dogfood/frankl-search/generator8_frontier.rad" `
        "--experimental-laws" "--record" $generatorEightFrontierTrace "--" `
        $generatorEightFrontierCertificate
    if ($LASTEXITCODE -ne 0) { throw "eight-generator graph frontier causal audit failed" }

    & $Rad "replay" $generatorEightFrontierTrace
    if ($LASTEXITCODE -ne 0) { throw "eight-generator graph frontier replay failed" }

    python "projects/dogfood/frankl-search/verify_projected_partition_frontier.py"
    if ($LASTEXITCODE -ne 0) { throw "projected quotient frontier verification failed" }

    & $Rad "projects/dogfood/frankl-search/projected_partition_proof.rad" `
        "--experimental-laws" "--record" $projectedPartitionTrace "--" `
        "projects/dogfood/frankl-search/certificates"
    if ($LASTEXITCODE -ne 0) { throw "projected quotient causal audit failed" }

    & $Rad "replay" $projectedPartitionTrace
    if ($LASTEXITCODE -ne 0) { throw "projected quotient proof replay failed" }

    python "projects/dogfood/frankl-search/verify_generator8_cnf_exclusions.py" `
        $generatorEightQ13Certificate
    if ($LASTEXITCODE -ne 0) { throw "eight-generator q=13 exclusion verification failed" }

    & $Rad "projects/dogfood/frankl-search/generator8_q13_exclusions.rad" `
        "--experimental-laws" "--record" $generatorEightQ13Trace "--" `
        $generatorEightQ13Certificate
    if ($LASTEXITCODE -ne 0) { throw "eight-generator q=13 causal audit failed" }

    & $Rad "replay" $generatorEightQ13Trace
    if ($LASTEXITCODE -ne 0) { throw "eight-generator q=13 replay failed" }

    if ($FullStructuralVerification) {
        python "projects/dogfood/frankl-search/regular_maxsat_solver.py" `
            --width 13 --proper --compact `
            --output "projects/dogfood/frankl-search/out/regular-maxsat-proper-n13.recomputed.json"
        if ($LASTEXITCODE -ne 0) { throw "exact regular-class optimization replay failed" }
    }
}
finally {
    Pop-Location
}
