# Nodus

**Node-based virtual audio router for Windows.** A canvas of nodes instead of a
table-style mixer — connect audio sources (apps, microphone, system) to outputs
(headphones, OBS, a virtual microphone) with wires, and control volume, mute,
balance and effects **per route**.

> 🌐 [Русская версия](README.ru.md) · 🆓 Nodus is free (donations only)
> · 🛠️ In active development

---

## What it does

- **Visual routing** — drag sources and outputs onto a canvas and draw wires
  between them. No fixed mixer rows.
- **Per-route control** — volume, mute and stereo balance live on each wire, not
  just on the source. The same source can feed several outputs, each with its
  own settings.
- **Mixer & Splitter** — combine many inputs into one (Mixer) or fan one input
  out to many (Splitter).
- **Virtual devices** — Nodus virtual microphones/outputs that other apps
  (Discord, OBS, games) see as real devices.
- **App auto-detection** — running audio apps (Spotify, Discord, Chrome, games…)
  appear automatically, with their real icons.
- **Scenes** — keep multiple routing setups and switch between them.

### Example

```
Game ─┬─→ Headphones        (you hear it)
      └─→ Nodus Virtual Out (OBS hears it)

Microphone ─┐
Spotify ────┴─→ Mixer ─→ Nodus Virtual Mic ─→ Discord
```

---

## Status

The audio engine MVP works on real hardware (WASAPI capture/render, per-route
volume/mute/pan, splitter, mixer, app isolation via process loopback). Virtual
microphones/outputs route through VB-Audio or the Nodus kernel driver when
installed.

In progress (Phase 2): dynamic creation of virtual devices from the UI (kernel
driver), real DSP for effects (currently pass-through), logic/trigger nodes and
push-to-talk.

---

## Tech stack

- **UI** — Tauri 1.x · React 18 · TypeScript · Vite
- **Backend** — Rust · WASAPI / Windows Core Audio
- **Virtual devices** — Windows kernel audio driver (SYSVAD-based)

## Development

> Requires Windows, Node.js, and the Rust toolchain.

```bash
npm install
npm run dev          # UI only, in the browser (http://localhost:1420)
npm run tauri dev    # the full desktop app (real audio backend)
npm run build        # production UI build
```

Rust backend:

```bash
cd src-tauri
cargo check
cargo test
```

## Versioning

Nodus follows [Semantic Versioning](https://semver.org): `MAJOR.MINOR.PATCH`
(e.g. `0.5.0`). Releases are tagged `vX.Y.Z`. The pre-redesign iteration is
archived under the [`v0.4-legacy`](https://github.com/voidless-labs/Nodus/releases/tag/v0.4-legacy)
tag.

## License

Free to use. A license will be finalized before a public release. The kernel
driver is based on Microsoft's SYSVAD sample (MIT).
