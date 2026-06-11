# uninstall.ps1 - Remove the Nodus Virtual Audio driver.
# Run from an elevated PowerShell:
#   powershell -ExecutionPolicy Bypass -File .\uninstall.ps1

#Requires -RunAsAdministrator
$ErrorActionPreference = "Continue"
$here = Split-Path -Parent $MyInvocation.MyCommand.Path

# 1 - Remove the device node (if devcon is available).
$devcon = (Get-Command devcon.exe -ErrorAction SilentlyContinue).Source
if (-not $devcon) {
    $devcon = (Get-ChildItem "C:\Program Files (x86)\Windows Kits\10\Tools" -Recurse -Filter devcon.exe -ErrorAction SilentlyContinue |
               Where-Object { $_.FullName -match "\\x64\\" } | Select-Object -First 1).FullName
}
if ($devcon) {
    Write-Host "Removing device node ROOT\NodusVirtualAudio..."
    & $devcon remove "ROOT\NodusVirtualAudio"
}

# 2 - Find and delete the staged driver package (oem*.inf) by original name.
Write-Host "Locating staged driver package..."
$published = pnputil /enum-drivers | Out-String
$oem = $null
$current = $null
foreach ($line in ($published -split "`r?`n")) {
    if ($line -match "Published Name\s*:\s*(oem\d+\.inf)") { $current = $matches[1] }
    if ($line -match "Original Name\s*:\s*nodus_audio\.inf" -and $current) { $oem = $current }
}
if ($oem) {
    Write-Host "Deleting driver package $oem ..."
    pnputil /delete-driver $oem /uninstall /force
} else {
    Write-Warning "nodus_audio.inf package not found in the driver store (already removed?)."
}

# 3 - Remove the test certificate (best effort).
Get-ChildItem Cert:\LocalMachine\Root, Cert:\LocalMachine\TrustedPublisher -ErrorAction SilentlyContinue |
    Where-Object { $_.Subject -eq "CN=Nodus Test Certificate" } |
    ForEach-Object {
        Write-Host "Removing test certificate from $($_.PSParentPath | Split-Path -Leaf)..."
        Remove-Item $_.PSPath -Force -ErrorAction SilentlyContinue
    }

Write-Host "`nDone. Reboot to fully unload the driver." -ForegroundColor Green
