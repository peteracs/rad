param(
    [string]$OutputDirectory = "target/riskbridge-fixtures"
)

$ErrorActionPreference = "Stop"
$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$source = Join-Path $repoRoot "projects/dogfood/riskbridge/plugins/fixtures/fixture.c"
$include = Join-Path $repoRoot "adapters/native"
$output = Join-Path $repoRoot $OutputDirectory
New-Item -ItemType Directory -Force -Path $output | Out-Null

$modes = [ordered]@{
    wrong_abi = 1
    wrong_calling_convention = 2
    wrong_struct_size = 3
    wrong_field_offset = 4
    swapped_opaque_type = 5
    overlapping_layout = 6
    declared_io = 7
    crash = 8
    timeout = 9
    nondeterministic = 10
    missing_export = 11
    malformed_output = 12
    nonfinite_score = 13
    unknown_reason_flags = 14
    unknown_disposition = 15
    host_failure = 16
    host_handle = 17
}

foreach ($entry in $modes.GetEnumerator()) {
    $dll = Join-Path $output ("{0}.dll" -f $entry.Key)
    $modeDefine = "-DRISK_FIXTURE_MODE=$($entry.Value)"
    & x86_64-w64-mingw32-gcc.exe -shared -O2 -I "$include" `
        $modeDefine -o "$dll" "$source"
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to build RiskBridge fixture $($entry.Key)"
    }
}

Write-Output "riskbridge fixtures: $($modes.Count) built in $output"
