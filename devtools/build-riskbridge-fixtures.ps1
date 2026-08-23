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
    nondeterministic = 10
    missing_export = 11
    containment = 18
}

$compilerVersion = (& x86_64-w64-mingw32-gcc.exe -dumpfullversion -dumpversion) -join "`n"
if ($LASTEXITCODE -ne 0) {
    throw "Unable to identify the RiskBridge fixture compiler"
}
$header = Join-Path $include "rad_extension.h"
$inputIdentity = @(
    "riskbridge-fixtures/v2",
    (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash,
    (Get-FileHash -LiteralPath $header -Algorithm SHA256).Hash,
    $compilerVersion,
    "-shared -O2 -s -Wl,--no-insert-timestamp,--image-base,0x180000000",
    (($modes.GetEnumerator() | ForEach-Object { "$($_.Key)=$($_.Value)" }) -join ";")
) -join "|"
$hasher = [Security.Cryptography.SHA256]::Create()
try {
    $inputDigest = ([BitConverter]::ToString(
        $hasher.ComputeHash([Text.Encoding]::UTF8.GetBytes($inputIdentity))
    ) -replace '-', '').ToLowerInvariant()
} finally {
    $hasher.Dispose()
}
$stamp = Join-Path $output "inputs.sha256"
$outputsExist = @($modes.Keys | Where-Object {
    -not (Test-Path -LiteralPath (Join-Path $output "$_.dll"))
}).Count -eq 0
if ($outputsExist -and (Test-Path -LiteralPath $stamp) -and
    ([IO.File]::ReadAllText($stamp).Trim() -eq $inputDigest)) {
    Write-Output "riskbridge fixtures: $($modes.Count) current in $output"
    exit 0
}

$buildDirectory = Join-Path $output ".build-$PID"
New-Item -ItemType Directory -Path $buildDirectory | Out-Null
$compilers = foreach ($entry in $modes.GetEnumerator()) {
    $dll = Join-Path $output ("{0}.dll" -f $entry.Key)
    # GNU ld derives both the DLL export name and its default image base from
    # the output basename. Keep that basename stable inside a per-run
    # directory: a PID-suffixed filename makes byte-identical inputs produce
    # different binaries even with --no-insert-timestamp.
    $temporary = Join-Path $buildDirectory ("{0}.dll" -f $entry.Key)
    $startInfo = [Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = "x86_64-w64-mingw32-gcc.exe"
    $startInfo.Arguments = @(
        "-shared",
        "-O2",
        "-s",
        "-Wl,--no-insert-timestamp,--image-base,0x180000000",
        "-I `"$include`"",
        "-DRISK_FIXTURE_MODE=$($entry.Value)",
        "-o `"$temporary`"",
        "`"$source`""
    ) -join " "
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    $process = [Diagnostics.Process]::Start($startInfo)
    [pscustomobject]@{
        Name = $entry.Key
        Destination = $dll
        Temporary = $temporary
        Process = $process
        Stdout = $process.StandardOutput.ReadToEndAsync()
        Stderr = $process.StandardError.ReadToEndAsync()
    }
}

$failures = [Collections.Generic.List[string]]::new()
foreach ($compiler in $compilers) {
    $compiler.Process.WaitForExit()
    $stdout = $compiler.Stdout.GetAwaiter().GetResult()
    $stderr = $compiler.Stderr.GetAwaiter().GetResult()
    if ($compiler.Process.ExitCode -ne 0) {
        $failures.Add("Failed to build RiskBridge fixture $($compiler.Name):`n$stdout`n$stderr")
    }
}
if ($failures.Count -ne 0) {
    foreach ($compiler in $compilers) {
        Remove-Item -LiteralPath $compiler.Temporary -Force -ErrorAction SilentlyContinue
    }
    Remove-Item -LiteralPath $buildDirectory -Force -ErrorAction SilentlyContinue
    throw ($failures -join "`n")
}

foreach ($compiler in $compilers) {
    $unchanged = (Test-Path -LiteralPath $compiler.Destination) -and
        ((Get-FileHash -LiteralPath $compiler.Destination -Algorithm SHA256).Hash -eq
         (Get-FileHash -LiteralPath $compiler.Temporary -Algorithm SHA256).Hash)
    if ($unchanged) {
        Remove-Item -LiteralPath $compiler.Temporary -Force
    } else {
        Move-Item -LiteralPath $compiler.Temporary -Destination $compiler.Destination -Force
    }
}
[IO.File]::WriteAllText($stamp, "$inputDigest`n", [Text.UTF8Encoding]::new($false))
Remove-Item -LiteralPath $buildDirectory -Force

Write-Output "riskbridge fixtures: $($modes.Count) built in $output"
