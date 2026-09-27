# Installs the `lungo` command from a lungo release, verifying its SHA-256 digest against the
# release's SHA256SUMS.
#
#   irm https://github.com/jowharshamshiri/lungo/releases/latest/download/install.ps1 | iex
#
# $env:LUNGO_VERSION selects a release (default: the latest); $env:LUNGO_INSTALL_DIR the directory
# the command is installed in (default: %LOCALAPPDATA%\lungo\bin).
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$repo = 'jowharshamshiri/lungo'
$dir = if ($env:LUNGO_INSTALL_DIR) { $env:LUNGO_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'lungo\bin' }
if ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'X64') {
    throw 'lungo has no Windows release for this processor'
}
$target = 'x86_64-pc-windows-msvc'

$version = $env:LUNGO_VERSION
if (-not $version) {
    $latest = Invoke-WebRequest -UseBasicParsing -Uri "https://github.com/$repo/releases/latest" -MaximumRedirection 5
    $version = ($latest.BaseResponse.ResponseUri.AbsoluteUri -replace '.*/tag/v', '')
}
$base = "https://github.com/$repo/releases/download/v$version"
$archive = "lungo-$version-$target.zip"

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ([System.Guid]::NewGuid())
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    Invoke-WebRequest -UseBasicParsing -Uri "$base/$archive" -OutFile (Join-Path $tmp $archive)
    Invoke-WebRequest -UseBasicParsing -Uri "$base/SHA256SUMS" -OutFile (Join-Path $tmp 'SHA256SUMS')
    $expected = $null
    foreach ($line in Get-Content (Join-Path $tmp 'SHA256SUMS')) {
        $parts = $line -split '\s+'
        if ($parts.Count -ge 2 -and $parts[1] -eq $archive) { $expected = $parts[0] }
    }
    if (-not $expected) { throw "SHA256SUMS of lungo $version lists no $archive" }
    $actual = (Get-FileHash -Algorithm SHA256 (Join-Path $tmp $archive)).Hash.ToLowerInvariant()
    if ($actual -ne $expected) { throw "$archive has SHA-256 $actual, but the release lists $expected; nothing was installed" }
    Expand-Archive -Path (Join-Path $tmp $archive) -DestinationPath $tmp
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    Copy-Item (Join-Path $tmp "lungo-$version-$target\lungo.exe") (Join-Path $dir 'lungo.exe') -Force
    "installed lungo $version in $dir"
    if (-not (($env:Path -split ';') -contains $dir)) { "add $dir to PATH to run lungo" }
} finally {
    Remove-Item -Recurse -Force $tmp
}
