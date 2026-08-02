# RADTRACK rolling-migration demo (Tier-1 #3: the convergence receipt
# across a schema migration).
#
#   powershell -File projects/dogfood/radtrack/demo/run_rolling_demo.ps1
#
# 1. v1 server, alice (v1) pulls, edits, syncs -> digests agree (same schema)
# 2. server SHUTS DOWN and UPGRADES: upgrade_v2.rad migrates the world
#    (assignee -> owner, estimate derived) and a v2 server takes the port
# 3. alice — still on v1 — asks `digest`: raw digests now differ BY
#    CONSTRUCTION, but schema_digest exposes the skew and the client asks
#    the server to CERTIFY: the server migrates her bytes on ingest and
#    digests THAT view -> MATCH: converged across the version boundary
# 4. alice edits offline and asks again -> certified MISMATCH (a real
#    divergence is still reported truthfully)

$ErrorActionPreference = "Stop"
$root = Resolve-Path "$PSScriptRoot\..\..\.."
Set-Location $root
$rad = ".\target\debug\rad.exe"
$demo = "projects\dogfood\radtrack\demo"

if (Test-Path "$demo\wr") { Remove-Item -Recurse "$demo\wr" }
New-Item -ItemType Directory "$demo\wr" | Out-Null
foreach ($f in "rolling_world.radw", "rolling_world_v2.radw") {
    if (Test-Path "$demo\$f") { Remove-Item "$demo\$f" }
}

function Client($who, $dir, $session) {
    Write-Host "`n=== $who : $session ===" -ForegroundColor Cyan
    cmd /c "$rad projects\dogfood\radtrack\track.rad -- $who $demo/$dir < $demo\$session 2>nul"
}

# --- phase 1: v1 world, same schema on both sides
$server = Start-Process -FilePath $rad -ArgumentList @("projects\dogfood\radtrack\server.rad", "--", "$demo\rolling_world.radw") `
    -RedirectStandardOutput "$demo\rolling_v1_log.txt" -RedirectStandardError "$demo\rolling_v1_err.txt" `
    -PassThru -NoNewWindow
Start-Sleep -Seconds 2
try {
    Client "alice" "wr" "r1_alice.txt"
} finally {
    cmd /c "echo shutdown> $demo\_rs.txt && echo quit>> $demo\_rs.txt"
    cmd /c "$rad projects\dogfood\radtrack\track.rad -- alice $demo/wr < $demo\_rs.txt 2>nul" | Out-Null
    Remove-Item "$demo\_rs.txt"
    if (!$server.HasExited) { $server.WaitForExit(8000) | Out-Null }
    if (!$server.HasExited) { Stop-Process -Id $server.Id -Force }
}

# --- phase 2: the upgrade (v1 save -> v2 save)
Write-Host "`n=== server upgrade: v1 -> v2 ===" -ForegroundColor Yellow
cmd /c "$rad projects\dogfood\radtrack\upgrade_v2.rad -- $demo\rolling_world.radw 2>nul"

# --- phase 3+4: v2 server, v1 client certifies across the boundary
$server2 = Start-Process -FilePath $rad -ArgumentList @("projects\dogfood\radtrack\server_v2_digest.rad", "--", "$demo\rolling_world_v2.radw") `
    -RedirectStandardOutput "$demo\rolling_v2_log.txt" -RedirectStandardError "$demo\rolling_v2_err.txt" `
    -PassThru -NoNewWindow
Start-Sleep -Seconds 2
try {
    Client "alice" "wr" "r2_alice_cert.txt"
    Client "alice" "wr" "r3_alice_diverge.txt"
} finally {
    if (!$server2.HasExited) { $server2.WaitForExit(8000) | Out-Null }
    if (!$server2.HasExited) { Stop-Process -Id $server2.Id -Force }
}

Write-Host "`n=== v2 server log ===" -ForegroundColor Yellow
Get-Content "$demo\rolling_v2_log.txt"
