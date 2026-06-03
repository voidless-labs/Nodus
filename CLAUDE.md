# Nodus — Node-based Virtual Audio Router

Nodus — это десктопное приложение для гибкого визуального роутинга звука
на Windows. Вместо табличного микшера — холст с нодами, логические правила,
виртуальные устройства и эффекты на маршрутах.

---

## Разделение ответственности — CRITICAL

| Слой | Кто делает | Статус |
|---|---|---|
| Визуал UI — только то что видит пользователь | Разработчик (человек) | 🟡 В разработке |
| Rust backend / WASAPI / Virtual Devices | Claude Code | 🟡 В разработке |
| Подключение логики к визуалу UI | Claude Code | 🔴 Ожидает готовности визуала |
| Tauri invoke bridge (контракт UI ↔ Rust) | Claude Code | 🔴 Ожидает готовности визуала |

### Что это значит на практике

**Разработчик делает:**
- React компоненты — только внешний вид
- CSS, анимации, layout, дизайн-система
- Статичные/моковые данные для отображения
- Canvas, ноды, edges — только визуально

**Разработчик НЕ делает:**
- Никаких Tauri invoke вызовов
- Никакой бизнес-логики
- Никакого state management связанного с аудио
- Никаких обработчиков реальных данных от backend

**Claude Code делает:**
- Весь Rust backend
- WASAPI интеграция
- Виртуальные устройства
- Когда визуал готов — подключает логику:
  - Изучает актуальный UI код самостоятельно
  - Добавляет invoke вызовы в React компоненты
  - Подключает Zustand store к реальным данным из Rust
  - Заменяет моковые данные на реальные
  - Настраивает Tauri event listeners

**Claude Code НЕ делает:**
- Не меняет внешний вид компонентов
- Не трогает CSS и стили
- Не меняет layout и структуру JSX
- Только добавляет логику к уже готовому визуалу

---

## Dev Setup

```bash
cd <project-root>
npm run dev              # только UI в браузере (:1420)
npm run tauri dev        # полное Tauri приложение
cargo build              # только Rust backend
cargo test               # тесты Rust
```

---

## Стек

**UI (визуал, не трогать):**
Tauri 1.x · React 18 · TypeScript · Zustand + Immer · Radix UI · lucide-react

**Backend (зона Claude Code):**
Rust · WASAPI / Windows Core Audio API · Tauri invoke bridge

---

## Rust Backend — Архитектура

### Структура `src-tauri/`
```
src-tauri/
├── src/
│   ├── main.rs
│   ├── audio/
│   │   ├── mod.rs
│   │   ├── wasapi.rs        # WASAPI интеграция
│   │   ├── devices.rs       # enumerate устройств
│   │   ├── session.rs       # Audio Session Management
│   │   └── virtual_device.rs
│   ├── routing/
│   │   ├── mod.rs
│   │   ├── graph.rs         # audio routing graph
│   │   ├── node.rs          # типы нод на уровне backend
│   │   └── engine.rs        # routing engine
│   ├── detection/
│   │   ├── mod.rs
│   │   └── process.rs       # auto-detect .exe приложений
│   └── commands/
│       ├── mod.rs
│       └── bridge.rs        # все Tauri invoke команды
├── Cargo.toml
└── tauri.conf.json
```

### Принципы Rust backend
- Event-driven архитектура
- Никаких `.unwrap()` в продакшн коде — только `?` или явная обработка
- Все публичные команды регистрируются только в `commands/bridge.rs`
- Каждый модуль покрыт unit тестами
- Rust не знает о React компонентах — только типизированные данные

---

## Tauri Invoke Bridge

Контракт определяется Claude Code по мере разработки backend.
При интеграции с UI — Claude Code изучает актуальный код визуала
и добавляет вызовы не меняя внешний вид компонентов.

### Устройства
```rust
#[tauri::command]
async fn get_audio_devices() -> Result<Vec<AudioDevice>, String>

struct AudioDevice {
    id: String,
    name: String,
    device_type: DeviceType, // Input | Output | Virtual
    is_default: bool,
}
```

### Routing
```rust
#[tauri::command]
async fn apply_routing_graph(graph: RoutingGraph) -> Result<(), String>

#[tauri::command]
async fn set_route_mute(route_id: String, muted: bool) -> Result<(), String>

#[tauri::command]
async fn set_route_volume(route_id: String, volume: f32) -> Result<(), String>
```

### Процессы
```rust
#[tauri::command]
async fn get_running_audio_processes() -> Result<Vec<AudioProcess>, String>

struct AudioProcess {
    exe_name: String,
    pid: u32,
    display_name: String,
}
```

### События Rust → UI
```rust
app.emit_all("audio-devices-changed", payload)?;
app.emit_all("process-changed", payload)?;
app.emit_all("volume-levels", payload)?;
```

---

## Домен — Типы нод

Nodus оперирует следующими типами нод. Это доменные понятия —
актуальные цвета, константы и структура компонентов берутся из UI кода,
не из этого файла.

**Source** — источники звука:
Arma 3, Discord, TeamSpeak, Spotify, Browser, Microphone, System Audio

**Output** — физические и виртуальные выходы:
Headphones, Speakers, OBS Video Output, Discord Virtual Mic, Recording Output

**Splitter** — разделитель потока (1 вход → много выходов)

**Mixer** — смешивание потоков (много входов → 1 выход)

**Switcher** — переключение между источниками

**FX** — аудио эффекты на маршруте:
Compressor, Limiter, Noise Gate, Noise Suppression, EQ,
Reverb, Delay, Pitch Shift, Gain, Ducking, Low/High-pass Filter

**Logic** — условные ноды:
срабатывают если запущено приложение, активен canvas, идёт запись

**Trigger** — хоткеи и push-to-talk:
Mouse4, Mouse5, CapsLock, Hold, Toggle, PTT

**Virtual** — виртуальные устройства Nodus:
Nodus Virtual Mic, Nodus Virtual Output, Nodus Game Output

---

## Routing концепция (для backend)

**Per-route control** — mute/volume/effects применяются к конкретному
маршруту (edge), не только к source ноде:
```
Arma → Headphones: muted
Arma → OBS: active, volume 80%
```

**Source cloning** — один источник используется в нескольких маршрутах,
каждый со своими эффектами и громкостью:
```
Arma Copy A → Headphones (Clean)
Arma Copy B → OBS Output (EQ + Compressor)
```

---

## Auto-detect приложений

```
arma3_x64.exe       → Game (Source)
discord.exe         → Chat (Source)
ts3client_win64.exe → Voice (Source)
spotify.exe         → Music (Source)
chrome.exe          → Browser (Source)
obs64.exe           → OBS (Output target)
```

---

## Примеры сценариев

```
# OBS слышит игру, я — нет
Arma 3 → OBS Output          (active)
Arma 3 → Headphones          (muted)

# Discord слышит музыку вместо микрофона
Spotify → Mixer → Virtual Mic → Discord
Mic → muted

# Push-to-talk только для записи
Mic → Video Output            (только при удержании Mouse4)

# Автодетект canvas
if arma3_x64.exe + ts3client_win64.exe running → activate "Arma" Canvas
```

---

## MVP — Минимально рабочий продукт

Это граница MVP. Всё что за ней — Phase 2. Агент не уходит дальше
пока MVP не реализован и не протестирован полностью.

### 1. Audio Routing
- [ ] Базовый routing: Source → Output (реальный звук через WASAPI)
- [ ] Mute / Volume на уровне маршрута (per-route, не только source)
- [ ] Splitter: 1 источник → несколько выходов
- [ ] Mixer: несколько источников → 1 выход

### 2. Виртуальные устройства
- [ ] Virtual Mic — Nodus как микрофон для Discord, TeamSpeak, игр
- [ ] Virtual Output — OBS и другие программы слышат Nodus как выход
- [ ] Динамическое создание/удаление виртуальных устройств (3+)
      Не одно фиксированное — пользователь создаёт сколько нужно

### 3. Auto-detect процессов
- [ ] Определение запущенных аудио приложений по .exe имени
- [ ] Обновление в реальном времени (приложение запустилось/закрылось)
- [ ] Маппинг .exe → тип Source ноды (см. таблицу Auto-detect)

### Критерий готовности MVP
Следующий сценарий работает end-to-end без ручной настройки Windows:
```
Arma 3 (auto-detected) → Splitter
                          ├→ Headphones           (слышу я)
                          └→ Nodus Virtual Output  (слышит OBS)

Mic → Mixer → Nodus Virtual Mic → Discord
Spotify → Mixer (та же нода, своя громкость)
```

---

## Roadmap

### UI Визуал (разработчик) — не трогать
- [x] Дизайн-система и компоненты ✅ 31.05.2026
- [x] Canvas + ноды + edges ✅ 31.05.2026
- [x] Все типы нод визуально ✅ 31.05.2026
- [x] Inspector panel, BottomDock, Library ✅ 31.05.2026
- [x] Multi-scene вкладки, Quick Controls ✅ 31.05.2026

### Rust Backend MVP (Claude Code) — параллельно с UI
- [x] Структура проекта `src-tauri/` ✅ 30.05.2026
- [x] WASAPI — enumerate реальных аудио устройств ✅ 30.05.2026 (6 устройств на реальной машине)
- [x] Audio routing graph (graph.rs + engine.rs) ✅ 30.05.2026
- [x] Базовый routing Source → Output через WASAPI ✅ 30.05.2026 (Galaxy Buds → MIXLINE)
- [x] Per-route mute / volume ✅ 30.05.2026 (set_route_mute в реальном времени)
- [x] Splitter (1 → много) ✅ 30.05.2026 (через tokio broadcast fanout)
- [x] Mixer (много → 1) ✅ 30.05.2026 (Windows shared mode mixing)
- [x] Auto-detect процессов по .exe + real-time обновление ✅ 30.05.2026 (Discord/Spotify/Steam/Firefox/Edge)
- [x] Unit тесты для каждого модуля ✅ 30.05.2026 (30 тестов)
- [x] Virtual Mic + Virtual Output ✅ 30.05.2026 (обнаружение VB-Audio реализовано; routing через них работает если VB-Audio установлен)
- [ ] Динамическое создание/удаление виртуальных устройств (требует kernel driver — Phase 2)

### Интеграция MVP (Claude Code — по готовности UI)
- [x] Изучить актуальный UI код самостоятельно ✅ 31.05.2026
- [x] Подключить invoke вызовы к готовым компонентам ✅ 31.05.2026
- [x] Tauri event listeners (devices, processes) ✅ 31.05.2026
- [x] Замена моковых данных на реальные ✅ 31.05.2026 (Library показывает реальные устройства и процессы)
- [x] Vite проект: package.json, vite.config.js, ES-module портирование ✅ 31.05.2026
- [ ] Проверка критерия готовности end-to-end (требует `npm run tauri dev`)

### Phase 2 — После MVP
- [x] VU meter (volume levels events → UI) ✅ 03.06.2026 (бэкенд + UI listener + meter-fill)
- [x] Save/load routing graph в JSON ✅ 03.06.2026 (doExport/doImport всех сцен)
- [ ] Logic ноды (условия по процессам)
- [ ] Trigger ноды (хоткеи, PTT)
- [ ] Canvas система (несколько сцен)
- [ ] Пресеты
- [ ] FX ноды (EQ, Compressor, Gate и др.)
- [x] Solo доходит до движка ✅ 03.06.2026 (buildRoutingGraph учитывает effective mute)

---

## Правила для Claude Code

1. **Не трогать** UI: CSS, JSX структуру, layout, стили — никогда
2. До интеграции работать только в `src-tauri/`
3. При интеграции — изучить актуальный UI код самостоятельно,
   не опираться на устаревшие данные из этого файла
4. Никаких `.unwrap()` в продакшн коде
5. Все Tauri команды только через `commands/bridge.rs`
6. Каждый модуль покрывать unit тестами
7. Максимум 3 параллельных агента одновременно
8. После каждого этапа — обновить статус Roadmap в этом файле