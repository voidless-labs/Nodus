# Nodus Virtual Audio Driver

WaveRT PortCls kernel driver that creates **"Nodus Virtual Speaker"** in Windows
Sound Settings.  Apps route audio to it; Nodus reads from a shared ring buffer.

## Prerequisites

| Tool | Version | Notes |
|------|---------|-------|
| Windows 11 | 22H2+ | Driver signing requirement |
| Visual Studio 2022 | 17.x | C++ Desktop + Universal Windows Platform |
| WDK 10.0.26100.0 | Matching SDK | download from Microsoft |

**Install WDK:**  
https://learn.microsoft.com/en-us/windows-hardware/drivers/download-the-wdk

WDK installer adds kernel-mode headers (`km/portcls.h`, `km/ks.h`, …) and the
VS driver project templates.

---

## Build via CI (recommended)

GitHub Actions builds and test-signs the driver — no local WDK needed.
See [`.github/workflows/driver.yml`](../../../.github/workflows/driver.yml).

1. One-time: set a repo variable `EWDK_ISO_URL` to the Enterprise WDK ISO download URL
   (Settings → Secrets and variables → Actions → Variables). The EWDK download is gated
   by Microsoft and versioned, so it can't be hard-coded.
2. Run the **Build Nodus Virtual Audio Driver** workflow (or push to `src-tauri/driver/**`).
3. Download the `nodus_audio-driver-x64-Release` artifact. It contains:
   `nodus_audio.sys`, `nodus_audio.inf`, `nodus_audio.cat`, `nodus_test.cer`,
   and `install.ps1` / `uninstall.ps1`.

### Install (from the CI artifact)

```powershell
# Run as Administrator, in the extracted artifact folder.
# Test-signed builds need Test Mode first:  bcdedit /set testsigning on  (then reboot)
powershell -ExecutionPolicy Bypass -File .\install.ps1
```

`install.ps1` trusts the test cert, stages the driver (`pnputil`), and creates the
`ROOT\NodusVirtualAudio` device node (`devcon`). Uninstall with `uninstall.ps1`.

---

## Build locally

### 1 — Enable test signing (once, requires reboot)

```powershell
# Run as Administrator
bcdedit /set testsigning on
Restart-Computer
```

After reboot a "Test Mode" watermark appears in the bottom-right corner — that's normal.

### 2 — Build the driver

Open **Developer Command Prompt for VS 2022** (or the WDK MSBuild shell):

```powershell
cd src-tauri\driver\nodus_audio
msbuild nodus_audio.vcxproj /p:Configuration=Release /p:Platform=x64
```

Output: `x64\Release\nodus_audio.sys` + `nodus_audio.inf`

### 3 — Install

```powershell
# Run as Administrator
pnputil /add-driver nodus_audio.inf /install

# The device node must be created once (PnP root-enumerated):
devcon install nodus_audio.inf "ROOT\NodusVirtualAudio"
# devcon.exe is in: C:\Program Files (x86)\Windows Kits\10\Tools\x64\devcon.exe
```

After install, "Nodus Virtual Speaker" appears in **Sound Settings → Output devices**.

### 4 — Set an app to use it

Windows 11 Settings → System → Sound → App volume and device preferences  
Set e.g. Spotify → **Nodus Virtual Speaker** as output device.

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│  audiodg.exe (Windows Audio Engine)                              │
│    writes 48 kHz stereo f32 PCM into WaveRT cyclic buffer (100ms)│
└──────────────────────┬──────────────────────────────────────────┘
                       │ MmBuildMdlForNonPagedPool  
             ┌─────────▼──────────────────────┐
             │  nodus_audio.sys  (kernel)       │
             │  CMiniportWaveRTStream           │
             │  Timer DPC @ 10 ms:              │
             │    copy new frames →             │
             │    NODUS_RING_BUFFER (768 KB)    │
             └─────────┬──────────────────────┘
                       │ ZwCreateSection / Global\NodusVirtualAudio
             ┌─────────▼──────────────────────┐
             │  Nodus userspace  (Rust)         │
             │  VirtualCapture::start()         │
             │  OpenFileMappingW + MapViewOfFile│
             │  polls WriteBytes, reads f32     │
             │  → broadcast::Sender<AudioFrame> │
             └─────────┬──────────────────────┘
                       │ existing routing engine
             ┌─────────▼──────────────────────┐
             │  Galaxy Buds / MIXLINE / OBS…   │
             └────────────────────────────────┘
```

---

## Uninstall

```powershell
# Run as Administrator
pnputil /delete-driver nodus_audio.inf /uninstall /force
```

Turn off test signing if no longer needed:
```powershell
bcdedit /set testsigning off
Restart-Computer
```
