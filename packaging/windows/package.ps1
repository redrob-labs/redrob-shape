<#
.SYNOPSIS
  Windows archive: the exe (signed when Azure Artifact Signing is configured) plus licences, zipped.
.EXAMPLE
  ./packaging/windows/package.ps1 -Arch x64 -Target x86_64-pc-windows-msvc -Version 0.1.0
#>
param(
  [Parameter(Mandatory = $true)] [string] $Arch,
  [Parameter(Mandatory = $true)] [string] $Target,
  [Parameter(Mandatory = $true)] [string] $Version
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

if ($env:ARTIFACT_SIGNING_ACCOUNT) {
  & (Join-Path $PSScriptRoot 'sign.ps1') (Join-Path $stage 'redrob-shape.exe')
} else {
  Write-Output '::warning::Artifact Signing is not configured; redrob-shape.exe ships unsigned.'
}

$zip = Join-Path $release "$name.zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip
Write-Output "wrote dist/release/$name.zip"
