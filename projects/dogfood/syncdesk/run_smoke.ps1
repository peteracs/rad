# syncdesk distributed smoke: one server, three clients ("machines"),
# offline divergence, merge-on-push with a rad-written conflict policy,
# in-flight events over the wire, cross-machine why().
#
#   powershell -File projects/dogfood/syncdesk/run_smoke.ps1   (from the repo root)
#
# Choreography: bob and carol pull the pristine world immediately, then
# block on file barriers; alice edits and pushes first. That makes carol's
# push a genuine three-way conflict (server has bob's T-2, carol diverged
# from pre-bob state) — resolved by the policy in server.rad, not by hand.

$ErrorActionPreference = "Stop"
$root = Resolve-Path "$PSScriptRoot\..\.."
Set-Location $root
$rad = Join-Path $root "target\debug\rad.exe"
$dir = "projects\dogfood\syncdesk"

Remove-Item -ErrorAction SilentlyContinue "$dir\world.json", "$dir\alice.done", "$dir\bob.done", "$dir\server.log", "$dir\server.err", "$dir\alice.log", "$dir\bob.log", "$dir\carol.log"

Start-Process -FilePath $rad -ArgumentList "$dir\server.rad" -RedirectStandardOutput "$dir\server.log" -RedirectStandardError "$dir\server.err" -NoNewWindow | Out-Null
Start-Sleep -Seconds 2

function Start-Client($name) {
    Start-Process powershell -ArgumentList '-NoProfile', '-Command', "Get-Content $dir\session_$name.txt | & $rad $dir\client.rad 2>`$null > $dir\$name.log" -NoNewWindow -PassThru
}

$bob = Start-Client "bob"
$carol = Start-Client "carol"
Start-Sleep -Seconds 2

# Through the same subshell as the others: rad's checker warnings go to
# stderr, which ErrorActionPreference=Stop would otherwise turn fatal.
$alice = Start-Client "alice"
$alice.WaitForExit()
New-Item -ItemType File "$dir\alice.done" -Force | Out-Null
$bob.WaitForExit()
New-Item -ItemType File "$dir\bob.done" -Force | Out-Null
$carol.WaitForExit()

Write-Host "===== alice ====="; Get-Content "$dir\alice.log"
Write-Host "===== bob =====";   Get-Content "$dir\bob.log"
Write-Host "===== carol ====="; Get-Content "$dir\carol.log"
Write-Host "===== server ====="; Get-Content "$dir\server.log"
