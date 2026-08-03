param(
    [ValidateSet("debug", "release")]
    [string]$Profile = "release"
)

$ErrorActionPreference = "Stop"
$project = Split-Path -Parent $MyInvocation.MyCommand.Path
$manifest = Join-Path $project "Cargo.toml"
$arguments = @("build", "--manifest-path", $manifest)
if ($Profile -eq "release") { $arguments += "--release" }

if (($IsWindows -or $env:OS -eq "Windows_NT") -and
    ((rustup toolchain list) -match "stable-x86_64-pc-windows-gnu")) {
    & rustup run stable-x86_64-pc-windows-gnu cargo @arguments
} else {
    & cargo @arguments
}
if ($LASTEXITCODE -ne 0) { throw "native math kernel build failed" }

$extension = if ($IsWindows -or $env:OS -eq "Windows_NT") {
    "rad_dogfood_math_kernels.dll"
} elseif ($IsMacOS) {
    "librad_dogfood_math_kernels.dylib"
} else {
    "librad_dogfood_math_kernels.so"
}
$source = Join-Path $project "target/$Profile/$extension"
$output = Join-Path $project "out"
New-Item -ItemType Directory -Force -Path $output | Out-Null
$installed = Join-Path $output "rad_dogfood_math_kernels"
Copy-Item -LiteralPath $source -Destination $installed -Force
Copy-Item -LiteralPath $source -Destination "$installed.dll" -Force
Write-Host $installed
