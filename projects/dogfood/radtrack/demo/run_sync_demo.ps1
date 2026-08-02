# RADTRACK two-client offline-conflict demo.
#
#   powershell -File projects/dogfood/radtrack/demo/run_sync_demo.ps1 [-Record]
#
# 1. server starts fresh (seeded backlog), optionally under --record
# 2. alice and bob both PULL the same base, then go offline
# 3. alice: T-1 -> P1, assigns herself, adds a ticket, SYNCs (clean merge)
# 4. bob:   T-1 -> P4 (collides with alice), closes T-2 (doesn't), SYNCs
#    -> server surfaces a FieldConflict; bob keeps his value; RESOLVE merges
# 5. both worlds converge with the server, digests compared
param([switch]$Record)

$ErrorActionPreference = "Stop"
$root = Resolve-Path "$PSScriptRoot\..\..\.."
Set-Location $root
$rad = ".\target\debug\rad.exe"
$demo = "projects\dogfood\radtrack\demo"

# clean slate
foreach ($d in "wa2", "wb2") {
    if (Test-Path "$demo\$d") { Remove-Item -Recurse "$demo\$d" }
    New-Item -ItemType Directory "$demo\$d" | Out-Null
}
if (Test-Path "$demo\server_world.radw") { Remove-Item "$demo\server_world.radw" }

$serverArgs = @("projects\dogfood\radtrack\server.rad", "--", "$demo\server_world.radw")
if ($Record) {
    $serverArgs = @("--record", "$demo\incident.radr") + $serverArgs
    if (Test-Path "$demo\incident.radr") { Remove-Item "$demo\incident.radr" }
}
$server = Start-Process -FilePath $rad -ArgumentList $serverArgs `
    -RedirectStandardOutput "$demo\server_log.txt" -RedirectStandardError "$demo\server_err.txt" `
    -PassThru -NoNewWindow
Start-Sleep -Seconds 2

function Client($who, $dir, $session) {
    Write-Host "`n=== $who : $session ===" -ForegroundColor Cyan
    cmd /c "$rad projects\dogfood\radtrack\track.rad -- $who $demo/$dir < $demo\$session 2>nul"
}

try {
    Client "alice" "wa2" "a1_pull.txt"
    Client "bob"   "wb2" "a1_pull.txt"     # same base as alice
    Client "alice" "wa2" "a2_edit_sync.txt"
    Client "bob"   "wb2" "b2_conflict_sync.txt"

    # alice re-syncs to receive bob's resolved world, then shuts the server down
    Write-Host "`n=== alice : final sync + shutdown ===" -ForegroundColor Cyan
    cmd /c "echo sync> $demo\_tmp.txt && echo digest>> $demo\_tmp.txt && echo list all>> $demo\_tmp.txt && echo shutdown>> $demo\_tmp.txt && echo quit>> $demo\_tmp.txt"
    cmd /c "$rad projects\dogfood\radtrack\track.rad -- alice $demo/wa2 < $demo\_tmp.txt 2>nul"
    Remove-Item "$demo\_tmp.txt"
} finally {
    # graceful first: after BYE the server still persists the world and
    # writes the --record tape; killing it mid-write loses the tape
    if (!$server.HasExited) { $server.WaitForExit(8000) | Out-Null }
    if (!$server.HasExited) { Stop-Process -Id $server.Id -Force }
}

Write-Host "`n=== server log ===" -ForegroundColor Yellow
Get-Content "$demo\server_log.txt"
if ($Record) {
    Write-Host "`nincident tape: $demo\incident.radr ($((Get-Item "$demo\incident.radr").Length) B)"
}
