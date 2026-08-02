# syncdesk soak: one server, N clients hammering pull/edit/DPUSH cycles for
# a duration, with the server's memory sampled throughout. The receipt for
# "long-running processes pay a fixed cost": memory flat, latency flat,
# every push merged, world still answering at the end.
#
#   powershell -File projects/dogfood/syncdesk/run_soak.ps1 -DurationSec 3600 -Clients 3
#
# Outputs (in projects/dogfood/syncdesk/):
#   soak_<name>.log   per-client sample lines: S <cycle> <pull_ms> <push_ms> <delta_bytes> <verdict>
#   soak_memory.csv   timestamp, server working set (bytes), private bytes
#   soak_server.log   server stdout

param(
    [int]$DurationSec = 3600,
    [int]$Clients = 3,
    [switch]$Debug
)

$ErrorActionPreference = "Continue"
$root = Resolve-Path "$PSScriptRoot\..\.."
Set-Location $root
$rad = Join-Path $root "target\release\rad.exe"
if ($Debug -or -not (Test-Path $rad)) { $rad = Join-Path $root "target\debug\rad.exe" }
$dir = "projects\dogfood\syncdesk"

$names = @("alice", "bob", "carol", "dave", "erin", "frank") | Select-Object -First $Clients

Remove-Item -ErrorAction SilentlyContinue "$dir\world.json", "$dir\soak_server.log", "$dir\soak_server.err", "$dir\soak_memory.csv"
foreach ($n in $names) { Remove-Item -ErrorAction SilentlyContinue "$dir\soak_$n.log" }

Write-Host "soak: $Clients clients, $DurationSec s, binary: $rad"

$srv = Start-Process -FilePath $rad -ArgumentList "$dir\server.rad" -RedirectStandardOutput "$dir\soak_server.log" -RedirectStandardError "$dir\soak_server.err" -NoNewWindow -PassThru
Start-Sleep -Seconds 2
if ($srv.HasExited) { Write-Host "server failed to start"; Get-Content "$dir\soak_server.err"; exit 1 }

# NB: not "$clients" — PowerShell variables are case-insensitive and that
# would collide with the [int]$Clients parameter.
$procs = @()
foreach ($n in $names) {
    $procs += Start-Process powershell -ArgumentList '-NoProfile', '-Command', "echo '$DurationSec $n' | & '$rad' '$dir\soak_client.rad' 2>`$null > '$dir\soak_$n.log'" -NoNewWindow -PassThru
}

# Memory sampler: every 10 s while clients run.
"unix_s,working_set_bytes,private_bytes" | Out-File "$dir\soak_memory.csv" -Encoding ascii
$soakStart = Get-Date
$deadline = $soakStart.AddSeconds($DurationSec + 60)
$earlyExit = $false
while ((Get-Date) -lt $deadline) {
    $alive = @($procs | Where-Object { -not $_.HasExited })
    $p = Get-Process -Id $srv.Id -ErrorAction SilentlyContinue
    if ($null -eq $p) { Write-Host "server died mid-soak"; break }
    $now = [int][double]::Parse((Get-Date -UFormat %s))
    "$now,$($p.WorkingSet64),$($p.PrivateMemorySize64)" | Out-File "$dir\soak_memory.csv" -Append -Encoding ascii
    if ($alive.Count -eq 0) {
        # Clients exiting well before the deadline is a failed soak, not a
        # finished one (this caught ephemeral-port exhaustion at the 60 s
        # mark of a "1-hour" run that then reported FLAT from 5 samples).
        if (((Get-Date) - $soakStart).TotalSeconds -lt ($DurationSec * 0.9)) { $earlyExit = $true }
        break
    }
    Start-Sleep -Seconds 10
}

# Final liveness probe + shutdown via raw TCP (one request per connection).
function Invoke-Rpc($msg) {
    try {
        $c = New-Object System.Net.Sockets.TcpClient("127.0.0.1", 7777)
        $s = $c.GetStream()
        $b = [System.Text.Encoding]::UTF8.GetBytes("$msg`n")
        $s.Write($b, 0, $b.Length)
        $r = New-Object System.IO.StreamReader($s)
        $line = $r.ReadLine()
        $c.Close()
        return $line
    } catch { return "RPC-FAILED: $_" }
}

$list = Invoke-Rpc "LIST"
Write-Host "final LIST: $($list.Substring(0, [Math]::Min(200, $list.Length)))"
$bye = Invoke-Rpc "SHUTDOWN"
Write-Host "shutdown: $bye"
if ($bye -notmatch "BYE") {
    # Never leave a zombie server squatting on :7777 to poison the next run.
    Write-Host "shutdown RPC failed; killing server pid $($srv.Id)"
    Stop-Process -Id $srv.Id -Force -ErrorAction SilentlyContinue
}

# ---- Summary -------------------------------------------------------------
Write-Host ""
Write-Host "===== soak summary ====="
if ($earlyExit) {
    Write-Host "soak verdict: FAILED - clients exited before 90% of requested duration"
}
foreach ($n in $names) {
    $done = Select-String -Path "$dir\soak_$n.log" -Pattern "^DONE" | Select-Object -Last 1
    Write-Host "client $n : $($done.Line)"
}

$mem = Import-Csv "$dir\soak_memory.csv"
if ($mem.Count -ge 2) {
    $first = [double]$mem[0].working_set_bytes / 1MB
    # Skip warmup: compare the 25% mark against the end.
    $q = [int]($mem.Count / 4)
    $warm = [double]$mem[$q].working_set_bytes / 1MB
    $last = [double]$mem[-1].working_set_bytes / 1MB
    $peak = ($mem | ForEach-Object { [double]$_.working_set_bytes } | Measure-Object -Maximum).Maximum / 1MB
    Write-Host ("server memory: start {0:N1} MB, warm(25%) {1:N1} MB, end {2:N1} MB, peak {3:N1} MB" -f $first, $warm, $last, $peak)
    if ($warm -gt 0 -and ($last / $warm) -lt 1.25) {
        Write-Host "memory verdict: FLAT (end within 25% of warm steady state)"
    } else {
        Write-Host ("memory verdict: GREW (end {0:N2}x warm) - investigate" -f ($last / $warm))
    }
}

# Latency trend: mean push_ms over first vs last 10% of each client's samples.
foreach ($n in $names) {
    $samples = Select-String -Path "$dir\soak_$n.log" -Pattern "^S " | ForEach-Object { ($_.Line -split " ")[3] } | ForEach-Object { [double]$_ }
    if ($samples.Count -ge 20) {
        $k = [int]($samples.Count / 10)
        $head = ($samples | Select-Object -First $k | Measure-Object -Average).Average
        $tail = ($samples | Select-Object -Last $k | Measure-Object -Average).Average
        Write-Host ("client {0} push latency: first decile {1:N1} ms, last decile {2:N1} ms ({3} samples)" -f $n, $head, $tail, $samples.Count)
    }
}
Write-Host "===== end ====="
