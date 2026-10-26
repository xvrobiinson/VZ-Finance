# Builds the release binary, runs the tests and compiles the installer into dist\.
# Usage: powershell -ExecutionPolicy Bypass -File scripts\build-installer.ps1
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
Write-Host "Acervus Finance $version"

cargo test --release
if ($LASTEXITCODE -ne 0) { throw "tests failed" }
cargo build --release
if ($LASTEXITCODE -ne 0) { throw "release build failed" }

$iscc = @(
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $iscc) { throw "Inno Setup 6 not found. Install it with: winget install JRSoftware.InnoSetup" }

& $iscc "/DAppVersion=$version" installer\vz-finance.iss
if ($LASTEXITCODE -ne 0) { throw "installer build failed" }
Get-Item "dist\AcervusFinance-Setup-$version.exe" | Select-Object FullName, Length
