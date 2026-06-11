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

# 2 - Find and delete the staged driver package(s). Get-WindowsDriver is
#     locale-independent (pnputil text output is localized and unsafe to parse).
Write-Host "Locating staged driver package..."
$pkgs = @(Get-WindowsDriver -Online -ErrorAction SilentlyContinue |
    Where-Object { $_.OriginalFileName -like '*nodus_audio.inf' })
if ($pkgs.Count -gt 0) {
    foreach ($p in $pkgs) {
        Write-Host "Deleting driver package $($p.Driver) (v$($p.Version))..."
        pnputil /delete-driver $p.Driver /uninstall /force
    }
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
