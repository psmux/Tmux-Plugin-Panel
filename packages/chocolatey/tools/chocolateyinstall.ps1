# ============================================================================
# TEMPLATE ONLY - DO NOT PUSH THIS FILE DIRECTLY TO CHOCOLATEY
# ============================================================================
# The real chocolateyinstall.ps1 is generated at publish time with the correct
# SHA256 checksum by either:
#   - GitHub Actions:  .github/workflows/release.yml (publish-chocolatey job)
#   - Local publish:   scripts/publish-choco.ps1
#
# Both download the release zip, compute the hash, and generate this file.
# ============================================================================

$ErrorActionPreference = 'Stop'

$toolsDir = "$(Split-Path -Parent $MyInvocation.MyCommand.Definition)"
$url64 = 'https://github.com/marlocarlo/Tmux-Plugin-Panel/releases/download/v__VERSION__/tppanel-v__VERSION__-windows-x64.zip'

$packageArgs = @{
  packageName    = $env:ChocolateyPackageName
  unzipLocation  = $toolsDir
  url64bit       = $url64
  checksum64     = '__SHA256_COMPUTED_AT_PUBLISH_TIME__'
  checksumType64 = 'sha256'
}

Install-ChocolateyZipPackage @packageArgs

# Create shims for tppanel, tmuxplugins, and tmuxthemes
$tppanelPath = Join-Path $toolsDir "tppanel.exe"
$tmuxpluginsPath = Join-Path $toolsDir "tmuxplugins.exe"
$tmuxthemesPath = Join-Path $toolsDir "tmuxthemes.exe"

Install-BinFile -Name "tppanel" -Path $tppanelPath
Install-BinFile -Name "tmuxplugins" -Path $tmuxpluginsPath
Install-BinFile -Name "tmuxthemes" -Path $tmuxthemesPath
