# TASK-007 — Tauri Commands Bridge

## Что сделано

`commands/bridge.rs` реализует полный контракт из CLAUDE.md:

### Зарегистрированные команды

| Команда | Описание | Статус |
|---------|----------|--------|
| `get_audio_devices` | Список Input/Output/Virtual устройств | ✅ |
| `get_running_audio_processes` | Список аудио процессов по .exe | ✅ |
| `apply_routing_graph` | Применить граф роутинга | ✅ |
| `set_route_mute` | Mute/unmute конкретного маршрута | ✅ |
| `set_route_volume` | Volume [0.0..1.0] конкретного маршрута | ✅ |
| `start_engine` | Запустить routing engine | ✅ |
| `stop_engine` | Остановить routing engine | ✅ |

### Events Rust → UI

| Событие | Payload | Статус |
|---------|---------|--------|
| `audio-devices-changed` | `Vec<AudioDevice>` | ✅ (при старте) |
| `process-changed` | `Vec<AudioProcess>` | ✅ (polling 2сек) |

### State management

```rust
EngineState(Mutex<RoutingEngine>)   // регистрируется в main.rs builder
DetectorState(Mutex<ProcessDetector>)
```

### setup_background_tasks()

Запускает ProcessDetector polling (2 секунды) и отправляет начальный список устройств.

## Тесты сериализации

```
audio_device_serializes ✅
audio_process_serializes ✅  
routing_graph_round_trips ✅
```

## Готовность к интеграции с UI

**Что нужно от разработчика:**
1. Готовый React компонент с местами для данных
2. Claude Code изучит актуальный UI код
3. Добавит `invoke("get_audio_devices")` и т.д.
4. Подключит события через `listen("audio-devices-changed", ...)`
5. Заменит моковые данные на реальные

**Что уже готово в Rust:**
- Все команды зарегистрированы
- Все типы сериализуются/десериализуются
- Background tasks запускаются при старте app

## Замечание по volume_levels

В CLAUDE.md есть `app.emit_all("volume-levels", payload)` — это Phase 2 (VU meter).
Не реализовано в MVP. Требует непрерывного polling уровней из IAudioMeterInformation.
