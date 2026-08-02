# RADTACTICS two-machine arena: one referee server, two clients that each
# command one side, think with the fork-AI on their own cpu, and ship orders
# as fork_delta bytes. The receipt for "distributed by construction":
# the wire carries divergence (hundreds of bytes), not worlds (tens of KB).
#
#   powershell -File projects/dogfood/tactics/run_arena.ps1
#
# Outputs (in projects/dogfood/tactics/):
#   arena_server.log   referee: boards per round, tamper/refusal lines
#   arena_red.log      red client: think time + delta size per round
#   arena_blue.log     blue client: same

param(
    [switch]$Debug,
    # Red also writes hp=999 into every delta it pushes; the referee reads
    # Intents only, logs "TAMPER", and the battle proceeds untouched.
    [switch]$Tamper,
    # Record the referee's run to arena_tape.radr, then replay it offline
    # (no sockets, no clients) and diff against the live log. The whole
    # networked match, reproduced from the referee's seat.
    [switch]$Record,
    [int]$TimeoutSec = 600
)

$ErrorActionPreference = "Continue"
$root = Resolve-Path "$PSScriptRoot\..\.."
Set-Location $root
$rad = Join-Path $root "target\release\rad.exe"
if ($Debug -or -not (Test-Path $rad)) { $rad = Join-Path $root "target\debug\rad.exe" }
$dir = "projects\dogfood\tactics"

Remove-Item -ErrorAction SilentlyContinue "$dir\arena_server.log", "$dir\arena_server.err", "$dir\arena_red.log", "$dir\arena_blue.log", "$dir\arena_tape.radr", "$dir\arena_replay.log", "$dir\arena_replay.err"

Write-Host "arena: binary $rad"
$srvArgs = @("$dir\arena_server.rad")
if ($Record) { $srvArgs += @("--record", "$dir\arena_tape.radr") }
$srv = Start-Process -FilePath $rad -ArgumentList $srvArgs -RedirectStandardOutput "$dir\arena_server.log" -RedirectStandardError "$dir\arena_server.err" -NoNewWindow -PassThru
Start-Sleep -Seconds 2
if ($srv.HasExited) { Write-Host "server failed to start:"; Get-Content "$dir\arena_server.err"; exit 1 }

$redLine = "red"
if ($Tamper) { $redLine = "red tamper" }
$procs = @()
$procs += Start-Process powershell -ArgumentList '-NoProfile', '-Command', "echo '$redLine' | & '$rad' '$dir\arena_client.rad' 2>`$null > '$dir\arena_red.log'" -NoNewWindow -PassThru
$procs += Start-Process powershell -ArgumentList '-NoProfile', '-Command', "echo blue | & '$rad' '$dir\arena_client.rad' 2>`$null > '$dir\arena_blue.log'" -NoNewWindow -PassThru

$deadline = (Get-Date).AddSeconds($TimeoutSec)
while ((Get-Date) -lt $deadline) {
    $alive = @($procs | Where-Object { -not $_.HasExited })
    if ($alive.Count -eq 0) { break }
    Start-Sleep -Seconds 2
}
foreach ($p in $procs) { if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue } }
if (-not $srv.HasExited) {
    Start-Sleep -Seconds 2
    if (-not $srv.HasExited) { Stop-Process -Id $srv.Id -Force -ErrorAction SilentlyContinue }
}

Write-Host ""
Write-Host "===== red client ====="
Get-Content "$dir\arena_red.log"
Write-Host ""
Write-Host "===== blue client (last 6 lines) ====="
Get-Content "$dir\arena_blue.log" | Select-Object -Last 6
Write-Host ""
Write-Host "===== referee (last 14 lines) ====="
Get-Content "$dir\arena_server.log" | Select-Object -Last 14

if ($Record) {
    Write-Host ""
    Write-Host "===== replaying the tape (offline, no sockets) ====="
    if (-not (Test-Path "$dir\arena_tape.radr")) { Write-Host "no tape was written"; exit 1 }
    $tapeKb = [math]::Round((Get-Item "$dir\arena_tape.radr").Length / 1KB, 1)
    Write-Host ("tape: arena_tape.radr ({0} KB)" -f $tapeKb)
    cmd /c "`"$rad`" replay `"$dir\arena_tape.radr`" > `"$dir\arena_replay.log`" 2> `"$dir\arena_replay.err`""
    Get-Content "$dir\arena_replay.err"
    $live = Get-Content "$dir\arena_server.log"
    $replayed = Get-Content "$dir\arena_replay.log"
    $diff = Compare-Object $live $replayed
    if ($null -eq $diff) {
        Write-Host ("REPLAY MATCHES LIVE RUN: {0} referee lines reproduced byte-for-byte" -f $live.Count)
    } else {
        Write-Host "REPLAY DIVERGED FROM LIVE LOG:"
        $diff | Select-Object -First 10 | Format-Table | Out-String | Write-Host
        exit 1
    }
}
