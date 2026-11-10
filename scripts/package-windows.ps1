# Builds a portable Windows release of ForgeTerm:
#   dist\ForgeTerm-<version>-windows-x64.zip  (+ .sha256)
#   dist\ForgeTerm-<version>-setup-x64.exe    (+ .sha256; only when NSIS 3 is available)
# containing forgeterm.exe, README.md, the font license and a generated list of
# third-party crates with their licenses. Installs nothing; writes only to dist\.
#
# Usage (from the repository root):  pwsh scripts\package-windows.ps1

param([string]$Makensis)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
}

$version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"([^"]+)"' |
    Select-Object -First 1).Matches[0].Groups[1].Value
$name = "ForgeTerm-$version-windows-x64"
$dist = Join-Path $root "dist"
$stage = Join-Path $dist $name

Write-Host "Building release $version ..."
cargo build --release -p forgeterm-app
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item target\release\forgeterm.exe $stage
Copy-Item README.md $stage
Copy-Item assets\fonts\JetBrainsMono-OFL.txt (Join-Path $stage "JetBrainsMono-OFL.txt")

# Every crate compiled into forgeterm.exe (normal dependencies), with its license.
$crates = cargo tree -p forgeterm-app -e normal --prefix none --format "{p}|{l}" |
    Where-Object { $_ -and $_ -notmatch '^forgeterm' } |
    ForEach-Object { ($_ -replace ' \(\*\)$', '').Trim() } |
    Sort-Object -Unique
$lines = @(
    "ForgeTerm $version - Created by Erik van de Blaak",
    "",
    "Third-party components compiled into forgeterm.exe (name version | license).",
    "Their source code is available unmodified on https://crates.io.",
    "Terminal font: JetBrains Mono, SIL Open Font License 1.1 (JetBrainsMono-OFL.txt).",
    ""
) + ($crates | ForEach-Object { $_ -replace '\|', '  |  ' })
Set-Content -Path (Join-Path $stage "THIRD-PARTY.txt") -Value $lines -Encoding utf8

$zip = Join-Path $dist "$name.zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
Compress-Archive -Path "$stage\*" -DestinationPath $zip
$hash = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLower()
Set-Content -Path "$zip.sha256" -Value "$hash  $name.zip" -Encoding ascii

Write-Host "Created $zip"
Write-Host "SHA-256 $hash"
Write-Host ("{0} third-party crates listed in THIRD-PARTY.txt" -f $crates.Count)

# Installer (per user, no admin rights) when NSIS 3 is available: on PATH, in its
# default install folder, or given with -Makensis / $env:MAKENSIS.
$makensis = @($Makensis, $env:MAKENSIS, (Get-Command makensis -ErrorAction SilentlyContinue).Source,
    "${env:ProgramFiles(x86)}\NSIS\makensis.exe", "$env:ProgramFiles\NSIS\makensis.exe") |
    Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
if (-not $makensis) {
    Write-Host "NSIS (makensis) not found: installer skipped. See README, 'Installeren'."
    return
}
# The icon build.rs rendered for forgeterm.exe.
$ico = Get-ChildItem target\release\build\forgeterm-app-*\out\forgeterm.ico |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not $ico) { throw "forgeterm.ico not found under target\release\build" }
$sidebar = Join-Path $dist "installer-sidebar.bmp"
& (Join-Path $PSScriptRoot "installer-art.ps1") -Out $sidebar
$setup = Join-Path $dist "ForgeTerm-$version-setup-x64.exe"
& $makensis /V2 /INPUTCHARSET UTF8 "/DVERSION=$version" "/DSTAGE=$stage" "/DICON=$($ico.FullName)" `
    "/DSIDEBAR=$sidebar" "/DOUTFILE=$setup" packaging\windows\forgeterm.nsi
if ($LASTEXITCODE -ne 0) { throw "makensis failed" }
$hash = (Get-FileHash -Algorithm SHA256 $setup).Hash.ToLower()
Set-Content -Path "$setup.sha256" -Value "$hash  $(Split-Path -Leaf $setup)" -Encoding ascii
Write-Host "Created $setup"
Write-Host "SHA-256 $hash"
