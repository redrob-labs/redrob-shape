<#
.SYNOPSIS
  Windows packages: the portable zip (exe signed when Azure Artifact Signing is configured, plus
  licences) and the branded setup exe (apps/redrob-shape-setup) with the signed app embedded,
  itself signed.
.EXAMPLE
  ./packaging/windows/package.ps1 -Arch x64 -Target x86_64-pc-windows-msvc -Version 0.1.0
#>
param(
  [Parameter(Mandatory = $true)] [string] $Arch,
  [Parameter(Mandatory = $true)] [string] $Target,
  [Parameter(Mandatory = $true)] [string] $Version,
  # Skip the setup exe (for a quick portable-only package).
  [switch] $NoSetup
)
$ErrorActionPreference = 'Stop'
$root = Resolve-Path (Join-Path $PSScriptRoot '..\..')
$bin = Join-Path $root "target\$Target\release\redrob-shape.exe"
if (-not (Test-Path $bin)) { throw "missing $bin" }
# "-portable.zip": the one zip name Redrob Launcher reads as an installable Windows build
# (`<stem>-<version>-<os>-<arch>-portable.zip`, parse_asset_name in redrob-launcher).
$name = "redrob-shape-$Version-windows-$Arch-portable"
$stage = Join-Path $root "dist\stage\$name"
$release = Join-Path $root 'dist\release'
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Force -Path $stage, $release | Out-Null
Copy-Item $bin (Join-Path $stage 'redrob-shape.exe')
foreach ($f in 'README.md', 'LICENSE-MIT', 'LICENSE-APACHE', 'NOTICE') { Copy-Item (Join-Path $root $f) $stage }

$signing = [bool] $env:ARTIFACT_SIGNING_ACCOUNT
if ($signing) {
  & (Join-Path $PSScriptRoot 'sign.ps1') (Join-Path $stage 'redrob-shape.exe')
} else {
  Write-Output '::warning::Artifact Signing is not configured; redrob-shape.exe ships unsigned.'
}

$zip = Join-Path $release "$name.zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip
Write-Output "wrote dist/release/$name.zip"

if ($NoSetup) { return }

# The setup embeds the SIGNED exe (build.rs reads REDROB_SHAPE_PAYLOAD), so it is built after
# signing; then the setup itself is signed. Settings > Apps shows the setup's own signature.
$env:REDROB_SHAPE_PAYLOAD = (Join-Path $stage 'redrob-shape.exe')
cargo build --release -p redrob-shape-setup --target $Target
if ($LASTEXITCODE -ne 0) { throw "redrob-shape-setup build failed" }
Remove-Item Env:\REDROB_SHAPE_PAYLOAD
$setupBin = Join-Path $root "target\$Target\release\redrob-shape-setup.exe"
if (-not (Test-Path $setupBin)) { throw "missing $setupBin" }
$setup = Join-Path $release "redrob-shape-$Version-windows-$Arch-setup.exe"
Copy-Item $setupBin $setup -Force
if ($signing) { & (Join-Path $PSScriptRoot 'sign.ps1') $setup }
Write-Output "wrote dist/release/redrob-shape-$Version-windows-$Arch-setup.exe"
