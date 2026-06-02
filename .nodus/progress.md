# Nodus — Progress

## Статус
🟡 Интеграция завершена — ожидает end-to-end теста через `npm run tauri dev`

## Текущая задача
Проверка критерия готовности MVP end-to-end.

## Выполнено

### ✅ TASK-001 — Scaffolding (30.05.2026)
Вся структура src-tauri/ создана с нуля. Cargo.toml, build.rs, tauri.conf.json,
все модули (audio, routing, detection, commands). 30 unit тестов.
Отчёт: .nodus/reports/001_foundation.md

### ✅ TASK-002 — WASAPI device enumeration (30.05.2026)
6 реальных устройств на реальной машине. Friendly names работают.
Отчёт: .nodus/reports/002_wasapi_integration.md

### ✅ TASK-003 — Process detection (30.05.2026)
5 уникальных аудио-процессов (Discord/Firefox/Edge/Spotify/Steam).
Дедупликация по exe-имени. ProcessDetector с 2-секундным polling.
Отчёт: .nodus/reports/002_wasapi_integration.md

### ✅ TASK-004 — Routing graph (30.05.2026)
Graph с Splitter/Mixer topology. resolve_device_routes() инлайнит промежуточные ноды.
Отчёт: .nodus/reports/001_foundation.md

### ✅ TASK-005 — WASAPI routing engine (30.05.2026)
Интеграционный тест: Galaxy Buds FE → MIXLINE.
Loopback capture + render работают. set_route_mute() в реальном времени.
Отчёт: .nodus/reports/002_wasapi_integration.md

### ✅ TASK-006 — Virtual devices (30.05.2026)
Обнаружение VB-Audio/Virtual Cable по имени. query_virtual_status() с инструкцией.
VB-Audio не установлен на машине — готово к работе когда будет установлен.
Отчёт: .nodus/reports/003_virtual_devices.md

### ✅ TASK-007 — Tauri bridge (30.05.2026)
Все 7 команд зарегистрированы. Events process-changed + audio-devices-changed работают.
State management через Builder::manage. Сериализация протестирована.
Отчёт: .nodus/reports/004_bridge.md

### ✅ TASK-008 — UI Integration (31.05.2026)
Прототип UI (Nodus_design_prototype/) портирован в Tauri Vite проект.
Созданы: package.json, vite.config.js, index.html, src/main.jsx
Конвертированы в ES-модули: graph-data.jsx, panels.jsx, canvas.jsx,
  tweaks-panel.jsx, app.jsx, styles.css
Добавлен src/tauri-bridge.js — graceful fallback когда не в Tauri.

Что подключено:
- Library: реальные устройства из get_audio_devices() (Input/Output/Virtual)
- Library: running status приложений из get_running_audio_processes()
- Library: кнопка rescan вызывает оба invoke
- Event listeners: audio-devices-changed, process-changed → обновление Library
- ENGAGE/STOP: start_engine() + apply_routing_graph() / stop_engine()
- Edge volume: set_route_volume(id, vol/100) в реальном времени
- Mute/delete nodes/edges: apply_routing_graph() с 300ms debounce
- dropDevice: сохраняет _deviceId (WASAPI ID) в ноде
- buildRoutingGraph: маппинг UI граф → Rust RoutingGraph
  (FX ноды → Mixer passthrough, trigger ноды — пропускаются)

Проверено: npm run build ✅ (42 модуля, 0 ошибок)
Проверено: npm run dev ✅ (UI рендерится, 0 ошибок в консоли)

## Цикл ремонта 1 (02.06.2026) — корректность и устойчивость backend

Аудит выявил проблемы MVP. В этом цикле исправлены три contained-фикса,
проверяемых через `cargo test` (33 теста зелёные, clippy без новых warning):

### ✅ FIX-001 — Коллизия route_id в Mixer (корректность)
`Source→Mixer→Output`: одно ребро Mixer→Output проходится по разу на каждый
вход, давая несколько `ActiveRoute` с одинаковым `route_id`. В движке
`routes: HashMap<RouteId, RouteHandles>` перезатирал записи → один вход
миксера молча терялся, его renderer утекал.
Исправлено: `HashMap<RouteId, Vec<RouteHandles>>`. set_route_mute/volume и
stop_internal обходят все handle под id. Добавлен тест
`mixer_paths_share_the_output_edge_route_id` (graph.rs).

### ✅ FIX-002 — Отравление мьютексов / правило "нет .unwrap()"
Все `lock().unwrap()` (engine.rs, bridge.rs, detection/process.rs) заменены на
`lock_recover()` (`unwrap_or_else(|e| e.into_inner())`). Паника под локом больше
не каскадит в панику всех последующих команд и фоновых потоков.
В продакшн-коде `lock().unwrap()` не осталось.

### ✅ FIX-003 — VU-поток блокировал движок 30 раз/сек
VU-цикл (bridge.rs, 33мс) брал блокирующий `lock`, конкурируя с apply_graph
(который держит лок через restart+sleep 80мс) и real-time слайдером громкости.
Заменено на `try_lock` со skip кадра при контеншене.

## Цикл ремонта 2 (02.06.2026) — снятие блокера #1 (виртуальное устройство)

### ✅ FIX-004 — CI для сборки/подписи драйвера — РАБОТАЕТ (02.06.2026)
`.github/workflows/driver.yml`: EWDK (mount ISO ~19 ГБ) → msbuild nodus_audio.vcxproj
→ self-signed test cert → signtool .sys → inf2cat (clean staging dir) → signtool .cat
→ upload артефакта (.sys/.inf/.cat/.cer + install/uninstall.ps1).
EWDK ISO URL = repo-переменная `EWDK_ISO_URL` (использован fwlink на EWDK build 28000).
✅ Прогон в реальном GitHub Actions ЗЕЛЁНЫЙ — артефакт nodus_audio-driver-x64-Release собирается.

Что пришлось чинить по ходу реальных прогонов (драйвер ни разу не собирался ранее):
- vcxproj: старый WDK 8.x формат → современный (Microsoft.Cpp.* + WindowsKernelModeDriver10.0);
  CI определяет версию kit и передаёт /p:WindowsTargetPlatformVersion.
- C++ ошибки PortCls: дубль NonDelegatingQueryInterface (DECLARE_STD_UNKNOWN), сигнатура
  NewStream, AllocateAudioBuffer/FreeAudioBuffer/GetHWLatency, поля PCPIN/PCFILTER дескрипторов,
  MAX_MINIPORTS→1, NonPagedPool→NonPagedPoolNx.
- Линковка: добавлен stdunk.lib (CUnknown), INITGUID в adapter.cpp (GUID'ы portcls).
- vcxproj: убран лишний _KERNEL_MODE (C4117), /WX off, SignMode=Off, EnableInf2Cat=false
  (встроенный inf2cat падал с пустым Configuration).
- inf: канонические SourceDisksNames/Files.
- CI: inf2cat в чистой staging-папке (только inf+sys); retry на букву диска после Mount-DiskImage.

⏳ Дальше (только на реальной машine): скачать артефакт → Test Mode → install.ps1 →
проверить «Nodus Virtual Speaker» в Sound Settings → ring-путь end-to-end.

### ✅ FIX-005 — Скрипты установки драйвера
`install.ps1` (импорт cert в Root+TrustedPublisher, проверка Test Mode,
pnputil /add-driver, devcon install ROOT\NodusVirtualAudio) и `uninstall.ps1`.

### ✅ FIX-006 — Интеграция VirtualCapture в движок
Добавлен `CaptureSource { Loopback | Virtual }` (engine.rs). Источник-нода с
именем "Nodus..." (детект через `virtual_device::is_nodus_virtual_name`) читается
из ring-буфера драйвера; при отсутствии ring — автоматический fallback на WASAPI
loopback (виртуальный спикер — реальный render-endpoint). Проброшен флаг
`from_is_virtual` через ActiveRoute/resolve. 33 теста зелёные, cargo check бинарей ок.
⚠️ Реальная работа ring-пути проверяется только с загруженным драйвером.

## Цикл ремонта 3 (03.06.2026) — изоляция звука приложений (#2 + #4)

### ✅ FIX-007 — WASAPI process loopback (per-app capture)
Источник-приложение больше НЕ снимает весь микс устройства. Добавлен
`ProcessLoopbackCapture` (session.rs) через `ActivateAudioInterfaceAsync` +
`AUDIOCLIENT_ACTIVATION_PARAMS` (PROCESS_LOOPBACK, INCLUDE_TARGET_PROCESS_TREE,
Win10 20348+). Захватывается только дерево процессов целевого PID.
- `find_audio_pid_for_exe()` — находит PID с активной аудио-сессией.
- Движок: `CaptureSource::ProcessLoopback`; для exe-источников выбирается process
  loopback (ключ `exe:<name>` для splitter fanout), для virtual — ring, иначе device loopback.
- Формат захвата = наш нормализованный (48k/2ch/f32): Windows ресэмплит звук приложения
  в него → попутно снимает часть проблемы #5 для app-источников.
- COM-хендлер завершения активации через `#[windows::core::implement]`
  (добавлены feature `implement` и крейт `windows-core`).

### ✅ FIX-008 — Per-route mute/volume больше не глушит приложение глобально (#4)
Убран весь `AppSessionControl`-плумбинг из движка (поля app_session в RouteHandles/
CaptureHandle, ветки в set_route_mute/volume). Теперь mute/volume применяются ТОЛЬКО к
нашей захваченной копии (атомики рендерера), не к Windows-сессии приложения. Это
корректный per-route: «Arma→OBS active, Arma→Headphones muted» больше не глушит Arma целиком.

Проверено: cargo build (lib+bins) ✅, 33 теста ✅, clippy без новых warning.
⚠️ Реальная работа process loopback (захват именно нужного PID, отсутствие тишины при
polling) проверяется только на реальной машине с воспроизводящимся приложением.

## Что осталось до полного MVP

### Требует `npm run tauri dev`:
- [ ] End-to-end тест сценария из CLAUDE.md (Arma 3 → Headphones + OBS Output)
- [ ] Проверить что реальные WASAPI устройства появляются в Library
- [ ] Проверить что запущенные процессы (Discord/Spotify) показываются как running
- [ ] Проверить что ENGAGE запускает реальный routing

### Требует VB-Audio Cable:
- [ ] Virtual Mic / Virtual Output routing
- [ ] "3+ виртуальных устройства" (через VB-Audio Hi-Fi Cable)

## Технические решения (зафиксировано)

### Rust backend
- windows-rs 0.58: STGM — newtype, STGM(0) для STGM_READ
- PROPVARIANT: raw bytes access (offset 0 = vt u16, offset 8 = PWSTR на x64)
- Virtual devices: через VB-Audio Cable detection (не kernel driver)
- Capture sharing: tokio broadcast channel, start() idempotent
- Process dedup: HashMap<lowercase_exe, AudioProcess>, первый PID, sort by display_name
- lib.rs: экспортирует модули для integration test binary (nodus-check)

### UI интеграция
- isTauri detection: window.__TAURI_IPC__ (Tauri v1, withGlobalTauri: false)
- Dynamic imports: @tauri-apps/api/tauri + @tauri-apps/api/event только в Tauri
- buildRoutingGraph: FX/gate/comp/eq/gain/duck → Mixer passthrough (MVP)
- trigger ноды пропускаются в routing graph (ctrl-only, не аудио)
- ctrl port edges пропускаются (duck ducking control, не аудио)
- applyGraphLater: debounce 300ms, читает nodesRef/edgesRef (всегда актуальны)
- setEdgeVol: set_route_volume напрямую (real-time, без debounce)
- Library prop defaults: INPUT_DEVICES / OUTPUT_DEVICES как fallback в browser mode

## Инструмент проверки

```bash
# Только Rust backend
cd src-tauri
cargo test --lib           # 30 unit тестов
cargo run --bin nodus-check  # интеграционный тест WASAPI

# Только UI (browser mode, без Tauri)
npm run dev                # http://localhost:1420

# Полное Tauri приложение
npm run tauri dev          # требует Tauri CLI
```

## Заблокировано
(нет)
