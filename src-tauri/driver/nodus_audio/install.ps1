# install.ps1 — Install the test-signed Nodus Virtual Audio driver.
# Run from an elevated PowerShell in the folder containing
# nodus_audio.sys / .inf / .cat / nodus_test.cer (the CI artifact).
#
#   powershell -ExecutionPolicy Bypass -File .\install.ps1
#
# Requires Test Mode for a test-signed driver:
#   bcdedit /set testsigning on   (then reboot)
# A release build signed with an EV / attestation cert does NOT need Test Mode.

#Requires -RunAsAdministrator
$ErrorActionPreference = "Stop"
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $here

$inf = Join-Path $here "nodus_audio.inf"
$cer = Join-Path $here "nodus_test.cer"
foreach ($f in @($inf, (Join-Path $here "nodus_audio.sys"))) {
    if (-not (Test-Path $f)) { throw "Missing required file: $f" }
}

# 1 — Warn if Test Mode is off (required for test-signed drivers).
$bcd = bcdedit /enum "{current}" | Out-String
if ($bcd -notmatch "(?im)^\s*testsigning\s+Yes") {
    Write-Warning "Test signing is OFF. A test-signed driver will fail to load."
    Write-Warning "Enable it (admin) then reboot:  bcdedit /set testsigning on"
}

# 2 — Trust the test certificate (Trusted Root + Trusted Publishers, machine store).
if (Test-Path $cer) {
    Write-Host "Importing test certificate into LocalMachine Root and TrustedPublisher..."
    Import-Certificate -FilePath $cer -CertStoreLocation Cert:\LocalMachine\Root          | Out-Null
    Import-Certificate -FilePath $cer -CertStoreLocation Cert:\LocalMachine\TrustedPublisher | Out-Null
} else {
    Write-Warning "nodus_test.cer not found — skipping cert import (ok for EV-signed builds)."
}

# 3 — Stage the driver package into the driver store.
Write-Host "Adding driver package via pnputil..."
pnputil /add-driver $inf /install
if ($LASTEXITCODE -ne 0) { throw "pnputil /add-driver failed ($LASTEXITCODE)" }

# 4 — Create the root-enumerated device node. pnputil cannot create a ROOT device,
#     so we use devcon from the WDK if available.
$devcon = Get-Command devcon.exe -ErrorAction SilentlyContinue
if (-not $devcon) {
    $candidates = Get-ChildItem "C:\Program Files (x86)\Windows Kits\10\Tools" -Recurse -Filter devcon.exe -ErrorAction SilentlyContinue |
                  Where-Object { $_.FullName -match "\\x64\\" } | Select-Object -First 1
    if ($candidates) { $devcon = $candidates.FullName } else { $devcon = $null }
} else { $devcon = $devcon.Source }

if ($devcon) {
    Write-Host "Creating device node ROOT\NodusVirtualAudio..."
    & $devcon install $inf "ROOT\NodusVirtualAudio"
    if ($LASTEXITCODE -ne 0) { throw "devcon install failed ($LASTEXITCODE)" }
    Write-Host "`nDone. 'Nodus Virtual Speaker' should appear in Sound Settings → Output." -ForegroundColor Green
} else {
    Write-Warning "devcon.exe not found. The driver package is staged but the device node was not created."
    Write-Warning "Install devcon (WDK) and run:  devcon install `"$inf`" `"ROOT\NodusVirtualAudio`""
}
