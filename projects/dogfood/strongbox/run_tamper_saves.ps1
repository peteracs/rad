# strongbox — save/load corruption driver.
#
#   powershell -File projects/dogfood/strongbox/run_tamper_saves.ps1
#
# load_world() aborts instead of returning a Result, so each corrupted save
# needs its own process. This driver runs every case and classifies the
# outcome by exit code:
#
#   REJECTED  the load failed and said why      (the contract holding)
#   ACCEPTED  the load succeeded                (correct only for S0)
#
# Run from the repo root.

$rad = Join-Path $PSScriptRoot "..\..\..\target\debug\rad.exe"
$prog = Join-Path $PSScriptRoot "tamper_save.rad"

$cases = @("S0","S1","S2","S3","S4","S5","S6","S7","S8","S9","S10","S11","S12","S13","S14")

# S0 is the control: it is the only case that may load.
$mustLoad = @("S0")

$rows = @()
$surprises = 0

foreach ($c in $cases) {
    $out = & $rad $prog -- $c 2>&1 | Out-String
    $code = $LASTEXITCODE

    $what = "?"
    if ($out -match "--- $c\: (.+?) ---") { $what = $Matches[1] }

    if ($code -eq 0) {
        $verdict = "ACCEPTED"
        $detail = ""
        if ($out -match "world_digest: (\w{12})") { $detail = "digest $($Matches[1])..." }
    } else {
        $verdict = "REJECTED"
        $line = ($out -split "`n" | Where-Object { $_ -match "Error|error:" } | Select-Object -First 1)
        $detail = ($line -replace "\s+", " ").Trim()
        if ($detail.Length -gt 96) { $detail = $detail.Substring(0, 96) + "..." }
    }

    $expected = if ($mustLoad -contains $c) { "ACCEPTED" } else { "REJECTED" }
    $flag = if ($verdict -eq $expected) { "  " } else { "!!"; }
    if ($verdict -ne $expected) { $surprises++ }

    $rows += [pscustomobject]@{
        Case = $c; Flag = $flag; Verdict = $verdict; What = $what; Detail = $detail
    }
}

Write-Host ""
Write-Host "=== strongbox: save/load corruption matrix ==="
foreach ($r in $rows) {
    Write-Host ("{0} {1,-4} {2,-9} {3}" -f $r.Flag, $r.Case, $r.Verdict, $r.What)
    if ($r.Detail) { Write-Host ("            {0}" -f $r.Detail) }
}
Write-Host ""
Write-Host ("=== {0} case(s) diverged from the expected verdict (marked !!) ===" -f $surprises)
