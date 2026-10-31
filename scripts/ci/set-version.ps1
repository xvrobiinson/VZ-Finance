# Stamps a release version into Cargo.toml and Cargo.lock before a CI build,
# so the executable, its Windows version info and the installer all carry it.
# The change stays on the build machine; it is never committed.
# Usage: pwsh scripts/ci/set-version.ps1 -Version 0.2.1
param([Parameter(Mandatory)][string]$Version)
$ErrorActionPreference = "Stop"

if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw "Not a MAJOR.MINOR.PATCH version: $Version" }
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)

function Set-First([string]$Path, [string]$Pattern, [string]$Replacement) {
    $text = [IO.File]::ReadAllText($Path)
    $regex = [regex]::new($Pattern, 'Multiline')
    if (-not $regex.IsMatch($text)) { throw "No version found in $Path" }
    [IO.File]::WriteAllText($Path, $regex.Replace($text, $Replacement, 1))
}

# The first `version =` line in Cargo.toml is the [package] one.
Set-First (Join-Path $root "Cargo.toml") '^version = "[^"]*"' "version = `"$Version`""
# Keep the lock file in step so `cargo build --locked` still passes.
Set-First (Join-Path $root "Cargo.lock") '(?<=^name = "acervus"\r?\nversion = ")[^"]*(?=")' $Version

Write-Host "Version set to $Version"
