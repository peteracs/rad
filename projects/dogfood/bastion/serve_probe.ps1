# Bounded `rad sandbox serve` probe: feed one JSONL request file on stdin,
# kill the server if it does not exit on its own. Never leaves a process
# running.
param(
    [Parameter(Mandatory = $true)][string]$Requests,
    [int]$TimeoutSec = 20
)

$rad = Join-Path $PSScriptRoot "..\..\..\target\debug\rad.exe"
$host_rad = Join-Path $PSScriptRoot "serve_host.rad"
$out = [System.IO.Path]::GetTempFileName()
$err = [System.IO.Path]::GetTempFileName()

$p = Start-Process -FilePath $rad `
    -ArgumentList @("sandbox", "serve", $host_rad) `
    -NoNewWindow -PassThru `
    -RedirectStandardInput $Requests `
    -RedirectStandardOutput $out -RedirectStandardError $err

if (-not $p.WaitForExit($TimeoutSec * 1000)) {
    $rss = [math]::Round($p.WorkingSet64 / 1MB, 1)
    $cpu = [math]::Round($p.CPU, 2)
    Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
    Write-Output "--- stdout ---"; Get-Content $out -ErrorAction SilentlyContinue
    Write-Output "--- stderr ---"; Get-Content $err -ErrorAction SilentlyContinue
    Write-Output "VERDICT: SERVER WEDGED (killed after ${TimeoutSec}s; cpu=${cpu}s rss=${rss}MB)"
} else {
    Write-Output "--- stdout ---"; Get-Content $out -ErrorAction SilentlyContinue
    Write-Output "--- stderr ---"; Get-Content $err -ErrorAction SilentlyContinue
    Write-Output "VERDICT: server exited $($p.ExitCode) cleanly"
}
Remove-Item $out, $err -ErrorAction SilentlyContinue
