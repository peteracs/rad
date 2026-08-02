# Timeout harness for adversarial probes: no probe may hang the session.
# Usage: .\run_timed.ps1 <file.rad> [timeoutSeconds]
param(
    [Parameter(Mandatory = $true)][string]$File,
    [int]$TimeoutSec = 30
)

$rad = Join-Path $PSScriptRoot "..\..\..\target\debug\rad.exe"
$out = [System.IO.Path]::GetTempFileName()
$err = [System.IO.Path]::GetTempFileName()
# Empty stdin, so a probe can never block waiting on input.
$in = [System.IO.Path]::GetTempFileName()

$sw = [System.Diagnostics.Stopwatch]::StartNew()
$p = Start-Process -FilePath $rad -ArgumentList $File -NoNewWindow -PassThru `
    -RedirectStandardOutput $out -RedirectStandardError $err `
    -RedirectStandardInput $in

if (-not $p.WaitForExit($TimeoutSec * 1000)) {
    $rss = [math]::Round($p.WorkingSet64 / 1MB, 1)
    $cpu = [math]::Round($p.CPU, 2)
    Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
    $sw.Stop()
    Get-Content $out -ErrorAction SilentlyContinue
    Get-Content $err -ErrorAction SilentlyContinue
    Write-Output "VERDICT: HUNG (killed after ${TimeoutSec}s; cpu=${cpu}s rss=${rss}MB)"
} else {
    $sw.Stop()
    Get-Content $out -ErrorAction SilentlyContinue
    Get-Content $err -ErrorAction SilentlyContinue
    $el = [math]::Round($sw.Elapsed.TotalSeconds, 2)
    Write-Output "VERDICT: exited $($p.ExitCode) in ${el}s"
}
Remove-Item $out, $err, $in -ErrorAction SilentlyContinue
