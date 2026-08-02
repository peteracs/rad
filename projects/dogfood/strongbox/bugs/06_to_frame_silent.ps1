# BUG 06 — `rad replay --to-frame N` silently ignores out-of-range N and
# still reports "Replay verified".
#
#   powershell -File projects\dogfood\strongbox\bugs\06_to_frame_silent.ps1
#
# Run projects\dogfood\strongbox\main.rad --record projects\dogfood\strongbox\run.radr first.
# The trace has 10 frames, so legal stop points are 0..10.

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..\..\..")).Path
$rad = Join-Path $repoRoot "target\debug\rad.exe"
$trace = Join-Path (Split-Path -Parent $PSScriptRoot) "run.radr"

Write-Host "=== rad replay --to-frame N, trace has 10 frames ==="
Write-Host ""
Write-Host "   N  exit  frames  outcome"

foreach ($n in @(0, 1, 2, 9, 10, 11, 50, 100000)) {
    $o = & $rad replay $trace --to-frame $n 2>&1 | Out-String
    $code = $LASTEXITCODE
    $frames = ([regex]::Match($o, 'Replay: (\d+) frame')).Groups[1].Value
    $stopped = ([regex]::Match($o, 'stopped at start of frame (\d+)')).Groups[1].Value

    if ($stopped -ne "") {
        $outcome = "stopped at frame $stopped   <- honoured"
    } elseif ($o -match 'Replay verified') {
        $outcome = "ran the WHOLE trace and printed 'Replay verified'   <- flag silently dropped"
    } else {
        $outcome = "?"
    }
    Write-Host ("{0,6}  {1,4}  {2,6}  {3}" -f $n, $code, $frames, $outcome)
}

Write-Host ""
Write-Host "EXPECTED: --to-frame 0 stops before frame 0 (runs nothing); any N > 10"
Write-Host "          is an error naming the trace's frame count."
Write-Host "ACTUAL:   0 and every N > 10 are discarded, the full trace runs, and the"
Write-Host "          tool reports success for a request it did not honour."
Write-Host ""
Write-Host "WHY IT MATTERS: bisecting a bug by binary search over frames is the"
Write-Host "          advertised workflow. Every probe past the end silently becomes"
Write-Host "          'run everything', so the bug appears present at every high"
Write-Host "          frame index and the search converges on the wrong answer."
