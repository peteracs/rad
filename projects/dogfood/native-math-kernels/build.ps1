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

$platform = if ($IsWindows -or $env:OS -eq "Windows_NT") {
    [pscustomobject]@{ Artifact = "rad_dogfood_math_kernels.dll"; Suffix = "dll" }
} elseif ($IsMacOS) {
    [pscustomobject]@{ Artifact = "librad_dogfood_math_kernels.dylib"; Suffix = "dylib" }
} else {
    [pscustomobject]@{ Artifact = "librad_dogfood_math_kernels.so"; Suffix = "so" }
}
$source = Join-Path $project "target/$Profile/$($platform.Artifact)"
$output = Join-Path $project "out"
New-Item -ItemType Directory -Force -Path $output | Out-Null
$installed = Join-Path $output "rad_dogfood_math_kernels"
Copy-Item -LiteralPath $source -Destination $installed -Force
Copy-Item -LiteralPath $source -Destination "$installed.$($platform.Suffix)" -Force
Write-Host $installed
