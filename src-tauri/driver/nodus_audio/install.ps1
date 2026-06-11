# install.ps1 - Install or update the test-signed Nodus Virtual Audio driver.
# Run from an elevated PowerShell in the folder containing
# nodus_audio.sys / .inf / .cat / nodus_test.cer (the CI artifact).
#
#   powershell -ExecutionPolicy Bypass -File .\install.ps1            # install / update if newer
#   powershell -ExecutionPolicy Bypass -File .\install.ps1 -Force     # reinstall even if same/older
#
# Behavior:
#   - not installed            -> fresh install (cert + driver + device node)
#   - installed, package newer -> update in place (may ask for a reboot)
#   - installed, same or older -> nothing to do (use -Force to override)
#
# Requires Test Mode for a test-signed driver:
#   bcdedit /set testsigning on   (then reboot)
# A release build signed with an EV / attestation cert does NOT need Test Mode.
#
# ASCII-only on purpose: avoids parser errors when the file is read under a
# non-UTF8 console codepage (Windows PowerShell 5.1).

#Requires -RunAsAdministrator
param([switch]$Force)

$ErrorActionPreference = "Stop"
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $here

$inf = Join-Path $here "nodus_audio.inf"
$cer = Join-Path $here "nodus_test.cer"
$hwid = "ROOT\NodusVirtualAudio"
foreach ($f in @($inf, (Join-Path $here "nodus_audio.sys"))) {
    if (-not (Test-Path $f)) { throw "Missing required file: $f" }
}

# --- helpers -----------------------------------------------------------------

# DriverVer of the package being installed, parsed from the .inf.
function Get-PackageVersion {
    $line = (Select-String -Path $inf -Pattern '^\s*DriverVer\s*=' | Select-Object -First 1).Line
    if ($line -and $line -match 'DriverVer\s*=\s*[0-9/]+\s*,\s*([0-9.]+)') {
        return [version]$matches[1]
    }
    throw "Could not parse DriverVer from $inf"
}

# All staged nodus_audio packages, locale-independent (pnputil text output is
# localized and unsafe to parse on non-English Windows).
function Get-InstalledPackages {
    return @(Get-WindowsDriver -Online -ErrorAction SilentlyContinue |
        Where-Object { $_.OriginalFileName -like '*nodus_audio.inf' })
}

function Test-DevicePresent {
    $dev = Get-PnpDevice -PresentOnly -ErrorAction SilentlyContinue |
        Where-Object { $_.HardwareID -contains $hwid }
    return [bool]$dev
}

function Find-Devcon {
    $d = (Get-Command devcon.exe -ErrorAction SilentlyContinue).Source
    if (-not $d) {
        $d = (Get-ChildItem "C:\Program Files (x86)\Windows Kits\10\Tools" -Recurse -Filter devcon.exe -ErrorAction SilentlyContinue |
              Where-Object { $_.FullName -match "\\x64\\" } | Select-Object -First 1).FullName
    }
    return $d
}

# Creates the device node when it does not exist yet (pnputil cannot create
# ROOT devices). Prefers devcon; falls back to the built-in hdwwiz wizard.
function New-DeviceNode {
    $devcon = Find-Devcon
    if ($devcon) {
        Write-Host "Creating device node $hwid via devcon..."
        & $devcon install $inf $hwid
        if ($LASTEXITCODE -ne 0) { throw "devcon install failed ($LASTEXITCODE)" }
    } else {
        Write-Warning "devcon.exe not found - the driver package is staged but no device node was created yet."
        Write-Host    "Create the device with the built-in wizard (no extra tools needed):" -ForegroundColor Cyan
        Write-Host    "  1. Run (admin):  hdwwiz"
        Write-Host    "  2. Next -> 'Install the hardware that I manually select (Advanced)'"
        Write-Host    "  3. 'Show All Devices' -> 'Have Disk...' -> browse to:"
        Write-Host    "       $inf"
        Write-Host    "  4. Select 'Nodus Virtual Audio' -> Next -> Finish."
        Start-Process hdwwiz.exe
    }
}

# --- decide what to do -------------------------------------------------------

$pkgVer    = Get-PackageVersion
$installed = Get-InstalledPackages
$instVer   = $null
if ($installed.Count -gt 0) {
    $instVer = ($installed | ForEach-Object { [version]$_.Version } |
                Sort-Object -Descending | Select-Object -First 1)
}

Write-Host "Package version  : $pkgVer"
Write-Host ("Installed version: " + $(if ($instVer) { "$instVer" } else { "(not installed)" }))

if ($instVer -and -not $Force -and $pkgVer -le $instVer) {
    Write-Host "Installed driver is already up to date - nothing to do." -ForegroundColor Green
    if (-not (Test-DevicePresent)) {
        Write-Host "Device node is missing though - creating it..."
        New-DeviceNode
    }
    return
}
if ($instVer) {
    if ($Force -and $pkgVer -le $instVer) {
        Write-Host "Force mode: reinstalling $pkgVer over $instVer."
    } else {
        Write-Host "Updating $instVer -> $pkgVer."
    }
}

# --- install / update --------------------------------------------------------

# 1 - Warn if Test Mode is off (required for test-signed drivers).
$bcd = bcdedit /enum "{current}" | Out-String
if ($bcd -notmatch "(?im)^\s*testsigning\s+Yes") {
    Write-Warning "Test signing is OFF. A test-signed driver will fail to load."
    Write-Warning "Enable it (admin) then reboot:  bcdedit /set testsigning on"
}

# 2 - Trust the test certificate (Trusted Root + Trusted Publishers, machine store).
if (Test-Path $cer) {
    Write-Host "Importing test certificate into LocalMachine Root and TrustedPublisher..."
    Import-Certificate -FilePath $cer -CertStoreLocation Cert:\LocalMachine\Root          | Out-Null
    Import-Certificate -FilePath $cer -CertStoreLocation Cert:\LocalMachine\TrustedPublisher | Out-Null
} else {
    Write-Warning "nodus_test.cer not found - skipping cert import (ok for EV-signed builds)."
}

# 3 - Stage the driver package; /install also updates any present device that
#     matches the hardware id. Exit code 3010 = success, reboot required.
Write-Host "Adding driver package via pnputil..."
pnputil /add-driver $inf /install
$rebootNeeded = ($LASTEXITCODE -eq 3010)
if ($LASTEXITCODE -ne 0 -and $LASTEXITCODE -ne 3010) {
    throw "pnputil /add-driver failed ($LASTEXITCODE)"
}

# 4 - Make sure the device node exists; on update give the device a nudge so it
#     rebinds to the new driver without waiting for a reboot (when possible).
if (Test-DevicePresent) {
    $devcon = Find-Devcon
    if ($devcon) {
        Write-Host "Updating existing device via devcon..."
        & $devcon update $inf $hwid
        if ($LASTEXITCODE -ne 0) {
            Write-Warning "devcon update returned $LASTEXITCODE - a reboot may be needed to finish the update."
            $rebootNeeded = $true
        }
    } else {
        pnputil /scan-devices | Out-Null
    }
} else {
    New-DeviceNode
}

# 5 - Clean up older staged copies of our package (keeps the driver store tidy).
foreach ($p in (Get-InstalledPackages)) {
    if ([version]$p.Version -lt $pkgVer) {
        Write-Host "Removing older staged package $($p.Driver) (v$($p.Version))..."
        pnputil /delete-driver $p.Driver /force | Out-Null
    }
}

if ($rebootNeeded) {
    Write-Warning "Reboot required to finish switching to the new driver version."
} else {
    Write-Host "`nDone. 'Nodus Virtual Speaker' should appear in Sound Settings -> Output." -ForegroundColor Green
}
