# install.ps1 - Install the test-signed Nodus Virtual Audio driver.
# Run from an elevated PowerShell in the folder containing
# nodus_audio.sys / .inf / .cat / nodus_test.cer (the CI artifact).
#
#   powershell -ExecutionPolicy Bypass -File .\install.ps1
#
# Requires Test Mode for a test-signed driver:
#   bcdedit /set testsigning on   (then reboot)
# A release build signed with an EV / attestation cert does NOT need Test Mode.
#
# ASCII-only on purpose: avoids parser errors when the file is read under a
# non-UTF8 console codepage (Windows PowerShell 5.1).

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

# 4 - Create the root-enumerated device node. pnputil cannot create a ROOT device.
#     Prefer devcon (WDK) if present; otherwise fall back to the built-in Add
#     Hardware wizard (hdwwiz), which needs no extra tools on a clean machine.
$devcon = (Get-Command devcon.exe -ErrorAction SilentlyContinue).Source
if (-not $devcon) {
    $devcon = (Get-ChildItem "C:\Program Files (x86)\Windows Kits\10\Tools" -Recurse -Filter devcon.exe -ErrorAction SilentlyContinue |
               Where-Object { $_.FullName -match "\\x64\\" } | Select-Object -First 1).FullName
}

if ($devcon) {
    Write-Host "Creating device node ROOT\NodusVirtualAudio via devcon..."
    & $devcon install $inf "ROOT\NodusVirtualAudio"
    if ($LASTEXITCODE -ne 0) { throw "devcon install failed ($LASTEXITCODE)" }
    Write-Host "`nDone. 'Nodus Virtual Speaker' should appear in Sound Settings -> Output." -ForegroundColor Green
} else {
    Write-Warning "devcon.exe not found - the driver package is staged but no device node was created yet."
    Write-Host    "Create the device with the built-in wizard (no extra tools needed):" -ForegroundColor Cyan
    Write-Host    "  1. Run (admin):  hdwwiz"
    Write-Host    "  2. Next -> 'Install the hardware that I manually select (Advanced)'"
    Write-Host    "  3. 'Show All Devices' -> 'Have Disk...' -> browse to:"
    Write-Host    "       $inf"
    Write-Host    "  4. Select 'Nodus Virtual Speaker' -> Next -> Finish."
    # Launch the wizard for convenience.
    Start-Process hdwwiz.exe
}
