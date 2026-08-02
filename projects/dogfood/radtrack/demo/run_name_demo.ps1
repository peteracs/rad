# RADTRACK name-claim demo (D3: name claims become resolvable).
#
#   powershell -File projects/dogfood/radtrack/demo/run_name_demo.ps1 [-Record]
#
# 1. server starts fresh (T-1, T-2 seeded; next ticket is T-3)
# 2. alice and bob both PULL the same base, then go offline
# 3. alice: `add` -> her tracker mints T-3, SYNC (clean merge; T-3 lands)
# 4. bob:   `add` -> HIS tracker also minted T-3 (same next_id, offline)
#    SYNC -> NameConflict: 'T-3' claimed by two entities
#    picker: "keep both? server's stays T-3/a, yours becomes T-3/b" -> yes
#    RESOLVE -> merge applies the renames and re-validates the claims
# 5. alice re-syncs; world_digest compared on all three machines
param([switch]$Record)

$ErrorActionPreference = "Stop"
$root = Resolve-Path "$PSScriptRoot\..\..\.."
Set-Location $root
$rad = ".\target\debug\rad.exe"
$demo = "projects\dogfood\radtrack\demo"

# clean slate
foreach ($d in "na", "nb") {
    if (Test-Path "$demo\$d") { Remove-Item -Recurse "$demo\$d" }
    New-Item -ItemType Directory "$demo\$d" | Out-Null
}
if (Test-Path "$demo\name_world.radw") { Remove-Item "$demo\name_world.radw" }

$serverArgs = @("projects\dogfood\radtrack\server.rad", "--", "$demo\name_world.radw")
if ($Record) {
    $serverArgs = @("--record", "$demo\name_incident.radr") + $serverArgs
    if (Test-Path "$demo\name_incident.radr") { Remove-Item "$demo\name_incident.radr" }
}
$server = Start-Process -FilePath $rad -ArgumentList $serverArgs `
    -RedirectStandardOutput "$demo\name_server_log.txt" -RedirectStandardError "$demo\name_server_err.txt" `
    -PassThru -NoNewWindow
Start-Sleep -Seconds 2

function Client($who, $dir, $session) {
    Write-Host "`n=== $who : $session ===" -ForegroundColor Cyan
    cmd /c "$rad projects\dogfood\radtrack\track.rad -- $who $demo/$dir < $demo\$session 2>nul"
}

try {
    Client "alice" "na" "a1_pull.txt"
    Client "bob"   "nb" "a1_pull.txt"      # same base as alice
    Client "alice" "na" "n2_alice_add.txt" # mints T-3, clean sync
    Client "bob"   "nb" "n3_bob_collide.txt" # also minted T-3 -> picker

    # alice re-syncs to receive the renames, then shuts the server down
    Write-Host "`n=== alice : final sync + shutdown ===" -ForegroundColor Cyan
    cmd /c "echo sync> $demo\_tmp.txt && echo digest>> $demo\_tmp.txt && echo list all>> $demo\_tmp.txt && echo shutdown>> $demo\_tmp.txt && echo quit>> $demo\_tmp.txt"
    cmd /c "$rad projects\dogfood\radtrack\track.rad -- alice $demo/na < $demo\_tmp.txt 2>nul"
    Remove-Item "$demo\_tmp.txt"
} finally {
    # graceful first: after BYE the server still persists and finishes the tape
    if (!$server.HasExited) { $server.WaitForExit(8000) | Out-Null }
    if (!$server.HasExited) { Stop-Process -Id $server.Id -Force }
}

Write-Host "`n=== server log ===" -ForegroundColor Yellow
Get-Content "$demo\name_server_log.txt"
if ($Record) {
    Write-Host "`nincident tape: $demo\name_incident.radr ($((Get-Item "$demo\name_incident.radr").Length) B)"
}
