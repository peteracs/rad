# RADSHEET offline-conflict demo (D5).
#
#   powershell -File projects/dogfood/radsheet/demo/run_sheet_demo.ps1 [-Record]
#
# 1. server starts fresh: budget sheet, B3 = SUM(B1:B2) = 1650
# 2. alice and bob PULL the same base, then go offline
# 3. alice: B1 -> 1300 (rent hike), why B3 (names alice), B4 = =B3*12,
#    SYNC (clean merge; server reflows B3 to 1750)
# 4. bob (from the ORIGINAL base): B1 -> 1250 (collides with alice),
#    B2 -> 500 (clean), SYNC -> conflict picker on cell B1; bob keeps his
#    -> RESOLVE merges, server reflows B3/B4 from merged raw
# 5. everyone re-syncs; world_digest equal on all three machines
param([switch]$Record)

$ErrorActionPreference = "Stop"
$root = Resolve-Path "$PSScriptRoot\..\..\.."
Set-Location $root
$rad = ".\target\debug\rad.exe"
$demo = "projects\dogfood\radsheet\demo"

foreach ($d in "wa", "wb") {
    if (Test-Path "$demo\$d") { Remove-Item -Recurse "$demo\$d" }
    New-Item -ItemType Directory "$demo\$d" | Out-Null
}
if (Test-Path "$demo\server_world.radw") { Remove-Item "$demo\server_world.radw" }

$serverArgs = @("projects\dogfood\radsheet\server.rad", "--", "$demo\server_world.radw")
if ($Record) {
    $serverArgs = @("--record", "$demo\sheet.radr") + $serverArgs
    if (Test-Path "$demo\sheet.radr") { Remove-Item "$demo\sheet.radr" }
}
$server = Start-Process -FilePath $rad -ArgumentList $serverArgs `
    -RedirectStandardOutput "$demo\server_log.txt" -RedirectStandardError "$demo\server_err.txt" `
    -PassThru -NoNewWindow
Start-Sleep -Seconds 2

function Client($who, $dir, $session) {
    Write-Host "`n=== $who : $session ===" -ForegroundColor Cyan
    cmd /c "$rad projects\dogfood\radsheet\sheet.rad -- $who $demo/$dir < $demo\$session 2>nul"
}

try {
    Client "alice" "wa" "s1_pull.txt"
    Client "bob"   "wb" "s1_pull.txt"      # same base as alice
    Client "alice" "wa" "s2_alice.txt"
    Client "bob"   "wb" "s3_bob.txt"

    Write-Host "`n=== alice : final sync + shutdown ===" -ForegroundColor Cyan
    cmd /c "echo sync> $demo\_tmp.txt && echo digest>> $demo\_tmp.txt && echo grid>> $demo\_tmp.txt && echo shutdown>> $demo\_tmp.txt && echo quit>> $demo\_tmp.txt"
    cmd /c "$rad projects\dogfood\radsheet\sheet.rad -- alice $demo/wa < $demo\_tmp.txt 2>nul"
    Remove-Item "$demo\_tmp.txt"
} finally {
    if (!$server.HasExited) { $server.WaitForExit(8000) | Out-Null }
    if (!$server.HasExited) { Stop-Process -Id $server.Id -Force }
}

Write-Host "`n=== server log ===" -ForegroundColor Yellow
Get-Content "$demo\server_log.txt"
if ($Record) {
    Write-Host "`ntape: $demo\sheet.radr ($((Get-Item "$demo\sheet.radr").Length) B)"
}
