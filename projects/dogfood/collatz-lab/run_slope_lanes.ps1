param(
    [string]$Rad = "target/release/rad.exe",
    [int]$Support = 11,
    [int]$Depth = 945,
    [int]$LaneCount = 256,
    [int]$ThrottleLimit = 16,
    [string]$OutputDirectory = "projects/dogfood/collatz-lab/out/slope11-lanes"
)

$ErrorActionPreference = "Stop"
& python (Join-Path $PSScriptRoot "run_slope_lanes.py") `
    --rad $Rad `
    --support $Support `
    --depth $Depth `
    --lanes $LaneCount `
    --workers $ThrottleLimit `
    --output $OutputDirectory
if ($LASTEXITCODE -ne 0) { throw "exact slope lane run failed" }
