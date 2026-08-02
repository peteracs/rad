# Diagnostics gallery: run every deliberately-illegal program in this folder
# and record what the checker says. Every file here MUST be rejected;
# an ACCEPTED verdict is a checker hole.
#
# Result against the shipped v0.5.0 binary (target\debug\rad.exe):
#   18 illegal programs, 1 wrongly ACCEPTED -> t17_all_arms_guarded.rad
#
# t17 is bug 07: a match whose arms are ALL guarded passed the exhaustiveness
# check and returned nil from a function declared `-> str`. That is fixed in
# core/vm/src/checker/typeck.rs, so a binary built from current source
# rejects t17 and this gallery reports 0 wrongly accepted. The shared
# target\debug\rad.exe predates the fix and still accepts it.
#
# Every other case is rejected with an accurate span and an actionable hint.
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..\..\..")).Path
$rad = Join-Path $repoRoot "target\debug\rad.exe"
$files = Get-ChildItem -Path $PSScriptRoot -Filter "t*.rad" | Sort-Object Name

$accepted = @()
foreach ($f in $files) {
    Write-Output ("=" * 70)
    Write-Output ("CASE  " + $f.Name)
    Write-Output ("=" * 70)
    $out = & $rad $f.FullName 2>&1 | Out-String
    Write-Output $out.Trim()
    if ($LASTEXITCODE -eq 0) {
        Write-Output ">>> VERDICT: ACCEPTED (exit 0) - checker hole if the program is illegal"
        $accepted += $f.Name
    } else {
        Write-Output ">>> VERDICT: rejected (exit $LASTEXITCODE)"
    }
    Write-Output ""
}

Write-Output ("=" * 70)
Write-Output ("SUMMARY: " + $files.Count + " illegal programs, " + $accepted.Count + " wrongly ACCEPTED")
foreach ($a in $accepted) { Write-Output ("  ACCEPTED: " + $a) }
