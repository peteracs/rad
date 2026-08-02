# strongbox — adversarial tests for recorded traces (.radr).
#
#   powershell -File projects\dogfood\strongbox\run_tamper_traces.ps1
#
# Record projects\dogfood\strongbox\main.rad first:
#   target\debug\rad.exe projects\dogfood\strongbox\main.rad --record projects\dogfood\strongbox\run.radr
#
# The docs promise three loud protection layers (builtins.md, "Replaying"):
#   1. Integrity        — embedded source checked against source_hash, a
#                         tampered trace is refused (override with --force)
#   2. Divergence       — every replayed io call checked against the trace
#   3. End-to-end       — final world digest compared to the recorded run
#
# A trace is `RADPACKZ:RADTRACE <64-hex digest> <zstd bytes>`, so every case
# below is a byte-level edit, the way a corrupt transfer would arrive.

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..\..")).Path
$rad = Join-Path $repoRoot "target\debug\rad.exe"
$src = Join-Path $PSScriptRoot "run.radr"
$tmp = Join-Path ([IO.Path]::GetTempPath()) "strongbox_traces"

New-Item -ItemType Directory -Force -Path $tmp | Out-Null

$bytes = [System.IO.File]::ReadAllBytes($src)
Write-Host "source trace: $($bytes.Length) bytes"

# Locate the envelope: tag SP digest SP body
$sp = @()
for ($i = 0; $i -lt 200; $i++) { if ($bytes[$i] -eq 32) { $sp += $i } }
$digestStart = $sp[0] + 1
$bodyStart = $sp[1] + 1
Write-Host "envelope: digest at $digestStart..$($sp[1]-1), body from $bodyStart"
Write-Host ""

function New-Case([string]$name, [byte[]]$data) {
    $p = Join-Path $tmp "$name.radr"
    [System.IO.File]::WriteAllBytes($p, $data)
    return $p
}

function Flip([byte[]]$src, [int]$at) {
    $c = $src.Clone()
    if ($c[$at] -eq 65) { $c[$at] = 66 } else { $c[$at] = 65 }
    return $c
}

$cases = @()

$cases += @{ n = "T0_clean"; p = New-Case "T0_clean" $bytes; expect = "verified" }
$cases += @{ n = "T1_digest_char_flipped"; p = New-Case "T1_digest_char_flipped" (Flip $bytes ($digestStart + 5)); expect = "refused" }
$cases += @{ n = "T2_body_byte_flipped"; p = New-Case "T2_body_byte_flipped" (Flip $bytes ($bodyStart + 60)); expect = "refused" }
$cases += @{ n = "T3_last_byte_flipped"; p = New-Case "T3_last_byte_flipped" (Flip $bytes ($bytes.Length - 3)); expect = "refused" }
$cases += @{ n = "T4_truncated_50"; p = New-Case "T4_truncated_50" $bytes[0..($bytes.Length - 51)]; expect = "refused" }
$cases += @{ n = "T5_header_only"; p = New-Case "T5_header_only" $bytes[0..($bodyStart + 4)]; expect = "refused" }
$cases += @{ n = "T6_garbage"; p = New-Case "T6_garbage" ([System.Text.Encoding]::ASCII.GetBytes("this is not a trace")); expect = "refused" }
$cases += @{ n = "T7_empty"; p = New-Case "T7_empty" ([byte[]]@()); expect = "refused" }

Write-Host "=== strongbox: recorded-trace tamper matrix ==="
$surprises = 0

foreach ($c in $cases) {
    $o = & $rad replay $c.p 2>&1 | Out-String
    $code = $LASTEXITCODE

    if ($o -match 'Replay verified') { $got = "verified" }
    else { $got = "refused" }

    $flag = "  "
    if ($got -ne $c.expect) { $flag = "!!"; $surprises++ }

    $msg = ($o -split "`n" | Where-Object { $_ -match "error|Error|refus|mismatch|digest|tamper" } | Select-Object -First 1)
    $msg = ($msg -replace "\s+", " ").Trim()
    if ($msg.Length -gt 100) { $msg = $msg.Substring(0, 100) + "..." }

    Write-Host ("{0} {1,-24} exit={2} {3,-9} {4}" -f $flag, $c.n, $code, $got.ToUpper(), $msg)
}

# The documented escape hatch must still work, and must still be honest.
Write-Host ""
Write-Host "=== --force on a tampered trace (documented override) ==="
$forced = & $rad replay ($cases | Where-Object { $_.n -eq "T2_body_byte_flipped" }).p --force 2>&1 | Out-String
Write-Host (($forced -split "`n" | Where-Object { $_.Trim() -ne "" } | Select-Object -First 4) -join "`n")

Write-Host ""
Write-Host ("=== {0} case(s) diverged from the expected verdict (marked !!) ===" -f $surprises)
