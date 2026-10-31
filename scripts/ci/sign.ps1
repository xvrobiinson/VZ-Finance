# Authenticode-signs files with the certificate in WINDOWS_SIGNING_CERT
# (base64 .pfx) and WINDOWS_SIGNING_PASSWORD, timestamped so signatures
# outlive the certificate. CI calls it only when the certificate is set.
# Usage: pwsh scripts/ci/sign.ps1 target\release\acervus.exe
param([Parameter(Mandatory, ValueFromRemainingArguments)][string[]]$Path)
$ErrorActionPreference = "Stop"

$signtool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" |
    Sort-Object FullName -Descending | Select-Object -First 1
if (-not $signtool) { throw "signtool.exe not found in the Windows SDK" }

$pfx = Join-Path $env:RUNNER_TEMP "signing.pfx"
try {
    [IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($env:WINDOWS_SIGNING_CERT))
    & $signtool.FullName sign /f $pfx /p $env:WINDOWS_SIGNING_PASSWORD /fd SHA256 `
        /tr http://timestamp.digicert.com /td SHA256 /d "Acervus Finance" $Path
    if ($LASTEXITCODE -ne 0) { throw "signing failed" }
    & $signtool.FullName verify /pa $Path
    if ($LASTEXITCODE -ne 0) { throw "signature verification failed" }
} finally {
    Remove-Item $pfx -Force -ErrorAction SilentlyContinue
}
