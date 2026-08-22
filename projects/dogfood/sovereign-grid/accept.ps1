[CmdletBinding()]
param(
    [string]$RadPath = "target/release/rad.exe",
    [string]$Output = "target/sovereign-grid-acceptance",
    [int]$BenchmarkSamples = 30,
    [int]$ModelRuns = 10000
)

$ErrorActionPreference = "Stop"
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
Set-Location $repoRoot

python.exe tooling/accept_sovereign_grid.py `
    --rad $RadPath `
    --output $Output `
    --benchmark-samples $BenchmarkSamples `
    --model-runs $ModelRuns
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}
