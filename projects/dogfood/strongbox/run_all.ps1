# strongbox — run everything, in order, from the repo root.
#
#   powershell -File projects\dogfood\strongbox\run_all.ps1
#
# Terminates on its own. Nothing here serves, listens, or waits for input.

$ErrorActionPreference = "Continue"
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..\..")).Path
$rad = Join-Path $repoRoot "target\debug\rad.exe"
$d = $PSScriptRoot

function Section($t) {
    Write-Host ""
    Write-Host ("=" * 72)
    Write-Host "  $t"
    Write-Host ("=" * 72)
}

Section "1. Schema evolution: gen1 -> gen2 -> gen3"
& $rad "$d\gen1_seed.rad"
& $rad "$d\gen2_migrate.rad"
& $rad "$d\gen3_migrate.rad" -- gen1
& $rad "$d\gen3_migrate.rad" -- gen2
Write-Host ""
Write-Host "  ^ the two gen3 world_digests above must be IDENTICAL:"
Write-Host "    migrating gen1->gen3 directly equals gen1->gen2->gen3 (confluence)."

Section "2. Round-trip contracts (save/load, fork wire bytes, RADPACK)"
& $rad "$d\roundtrip.rad"

Section "3. Wire-codec tamper harness (12 adversarial payloads)"
& $rad "$d\tamper_wire.rad"

Section "4. Save/load corruption matrix (15 cases, one process each)"
& powershell -NoProfile -File "$d\run_tamper_saves.ps1"

Section "5. The app: build and seal the archive"
& $rad "$d\main.rad"

Section "6. Verify the sealed archive against its receipt chain"
& $rad "$d\verify.rad"

Section "7. Forge four plausible archives; the receipt chain must catch each"
foreach ($m in @("none", "downgrade", "whitewash", "erase", "backdate")) {
    & $rad "$d\forge.rad" -- $m
    & $rad "$d\verify.rad" -- "forged_$m" 2>&1 | Select-String -Pattern "VERDICT"
    Write-Host ""
}

Section "8. Record and replay: the digest must match bit-for-bit"
& $rad "$d\main.rad" --record "$d\run.radr"
& $rad replay "$d\run.radr"

Section "9. Recorded-trace tamper matrix (8 byte-level corruptions)"
& powershell -NoProfile -File "$d\run_tamper_traces.ps1"

Section "10. Retroactive edit: a stricter SLA policy against the recorded week"
& $rad replay "$d\run.radr" --with "$d\fixed.rad"

Section "11. Known bugs, reproduced (non-zero exits below are the POINT)"
Write-Host "-- BUG 01: load_world type confusion --"
& $rad "$d\bugs\01_load_type_confusion.rad"
Write-Host "-- BUG 04b: float round-trip threshold --"
& $rad "$d\bugs\04b_float_threshold.rad"
Write-Host "-- BUG 06: replay --to-frame silently ignores out-of-range N --"
& powershell -NoProfile -File "$d\bugs\06_to_frame_silent.ps1"
Write-Host "-- BUG 08: rad test passes a file that asserts 1 == 2 --"
& $rad test "$d\bugs\08_dir"

Write-Host ""
Write-Host "=== strongbox: run_all complete ==="
