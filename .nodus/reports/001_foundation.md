# TASK-001 — Scaffolding: структура Tauri проекта

## Что сделано

Создана полная структура `src-tauri/` с нуля (проект был пустой — только CLAUDE.md).

Реализованы все модули с полными скелетами и unit тестами:
- **audio/wasapi.rs** — ComGuard (RAII COM init/uninit), AudioFormat (48kHz/2ch/f32)
- **audio/devices.rs** — WASAPI device enumeration через IMMDeviceEnumerator
- **audio/session.rs** — LoopbackCapture + AudioRenderer (WASAPI capture/render потоки)
- **audio/virtual_device.rs** — обнаружение VB-Audio и аналогов, VirtualDeviceStatus
- **routing/node.rs** — Node, Route типы с per-route volume/mute
- **routing/graph.rs** — Graph с add/remove/route/mute/volume + resolve_device_routes()
- **routing/engine.rs** — RoutingEngine (start/stop/apply_graph + live set_mute/set_volume)
- **detection/process.rs** — ProcessDetector с polling + classify_exe маппинг
- **commands/bridge.rs** — все Tauri invoke команды по контракту CLAUDE.md

## Созданные файлы

- `src-tauri/Cargo.toml` — зависимости: tauri 1.x, windows 0.58, tokio, serde, tracing, uuid, dashmap
- `src-tauri/build.rs` — tauri-build
- `src-tauri/tauri.conf.json` — конфиг приложения
- `src-tauri/icons/icon.ico` — placeholder 1×1 ICO
- `src-tauri/src/main.rs` — точка входа, регистрация команд и state
- `src-tauri/src/audio/{mod,wasapi,devices,session,virtual_device}.rs`
- `src-tauri/src/routing/{mod,node,graph,engine}.rs`
- `src-tauri/src/detection/{mod,process}.rs`
- `src-tauri/src/commands/{mod,bridge}.rs`
- `dist/index.html` — placeholder для tauri build (devPath = localhost, distDir нужен)

## Решения и компромиссы

**PROPVARIANT доступ**: windows-rs 0.58 использует `STGM` как newtype (`STGM(0)`), а PROPVARIANT имеет плоский бинарный layout. Используем raw pointer чтение: offset 0 = vt (u16), offset 8 = PWSTR для VT_LPWSTR (31). Это x64-специфично, но Nodus только для Windows x64.

**Virtual devices**: Настоящий виртуальный драйвер = kernel-mode driver. MVP решение — обнаруживаем VB-Audio/Virtual Audio Cable по имени, роутим через них. Динамическое создание через API невозможно без установки драйвера.

**Capture sharing (Splitter)**: LoopbackCapture использует tokio broadcast channel — второй вызов `start()` возвращает новый subscriber от существующего sender. Fanout бесплатный.

**Process detector lifetime**: ProcessDetector::start() клонирует Arc-ссылки в фоновый поток. Локальный struct можно дропать — поток продолжает работать через свои Arc.

## Тесты

```
cargo test -- 30 passed; 0 failed
```

Покрытие:
- routing/node: route_defaults_are_audible, node_gets_unique_ids
- routing/graph: add/remove nodes, duplicate routes, set_mute/volume, resolve_source_to_output, resolve_splitter_fanout
- routing/engine: start/stop state, double_start_returns_error, stop_without_start_returns_error, apply_graph
- detection/process: classify_exe (game/chat/music), unknown_exe, case_insensitive, detect_returns_vec_on_windows
- audio/wasapi: ComGuard::init, AudioFormat frame_size/bytes_per_second
- audio/devices: enumerate_returns_at_least_one_device (Windows), device_type_serializes
- audio/virtual_device: VB-Audio detection, find_virtual_devices, status_message
- audio/session: volume bit round-trip, clamp_volume
- commands/bridge: AudioDevice/AudioProcess/RoutingGraph serialization

## Известные ограничения

- Icons — placeholder 1×1 ICO (нужны реальные иконки для релиза)
- `session.rs` Windows реализация не покрыта автотестами (требует реального WASAPI устройства)
- PROPVARIANT читается через raw bytes — работает только на x64 Windows

## Следующий шаг

TASK-002 (WASAPI devices — уже реализован в devices.rs, нужна интеграционная проверка)
TASK-003 (Process detection — реализован, протестирован)
TASK-004 (Routing graph — реализован, протестирован)

По факту TASK-001 уже включает реализацию 002-004 на уровне скелета. Следующий реальный шаг — TASK-005 (engine integration) и TASK-007 (bridge).
