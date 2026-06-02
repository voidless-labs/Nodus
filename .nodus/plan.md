# Nodus Rust Backend — Plan

## Правила работы
- Максимум 3 параллельных агента
- После каждой задачи — отчёт в .nodus/reports/
- После каждой задачи — обновить .nodus/progress.md
- После каждой задачи — обновить Roadmap в CLAUDE.md (отметить выполненное)
- Никаких .unwrap() в продакшн коде
- Каждый модуль покрыт unit тестами

---

## Задачи

### TASK-001: Scaffolding — структура Tauri проекта
Описание: Создать полную структуру src-tauri/ — Cargo.toml с зависимостями,
  tauri.conf.json, build.rs, и скелеты всех модулей (main.rs, mod.rs файлы).
  Проект должен компилироваться без ошибок после этого шага.
Файлы:
  - src-tauri/Cargo.toml
  - src-tauri/build.rs
  - src-tauri/tauri.conf.json
  - src-tauri/src/main.rs
  - src-tauri/src/audio/mod.rs
  - src-tauri/src/routing/mod.rs
  - src-tauri/src/detection/mod.rs
  - src-tauri/src/commands/mod.rs
Зависимости: нет
Критерий готовности: `cargo build` проходит без ошибок
Тесты: базовый тест компиляции

### TASK-002: WASAPI — enumerate аудио устройств
Описание: Реализовать перечисление реальных аудио устройств через Windows WASAPI.
  Вернуть список устройств с типами Input/Output.
Файлы:
  - src-tauri/src/audio/wasapi.rs
  - src-tauri/src/audio/devices.rs
Зависимости: TASK-001
Критерий готовности: тест возвращает список реальных устройств системы
Тесты: unit тест enumerate_devices(), проверка что список не пустой на Windows

### TASK-003: Process detection — auto-detect аудио приложений
Описание: Определение запущенных аудио процессов по .exe имени через Windows API.
  Real-time обновление через polling. Маппинг exe → тип Source ноды.
Файлы:
  - src-tauri/src/detection/process.rs
  - src-tauri/src/detection/mod.rs
Зависимости: TASK-001
Критерий готовности: тест возвращает список процессов; при запуске/закрытии
  приложения список обновляется в течение 2 секунд
Тесты: unit тест маппинга exe→тип, mock тест обновления списка

### TASK-004: Audio routing graph — граф маршрутов
Описание: Реализовать граф маршрутизации: ноды (Source/Output/Splitter/Mixer),
  рёбра (Route с per-route mute/volume). Граф хранится в памяти.
Файлы:
  - src-tauri/src/routing/node.rs
  - src-tauri/src/routing/graph.rs
  - src-tauri/src/routing/mod.rs
Зависимости: TASK-001
Критерий готовности: можно добавить ноды, соединить маршрутами, изменить
  volume/mute на конкретном маршруте; тесты проходят
Тесты: unit тесты add_node, add_route, set_mute, set_volume, splitter topology

### TASK-005: WASAPI routing engine — реальный звук
Описание: Routing engine, который берёт аудио-поток из WASAPI capture (loopback
  или microphone) и рендерит в WASAPI render endpoint. Реализует:
  - Source → Output (базовый)
  - Per-route mute / volume
  - Splitter (1 источник → несколько выходов через клонирование буфера)
  - Mixer (несколько источников → 1 выход через суммирование с gain)
Файлы:
  - src-tauri/src/audio/session.rs
  - src-tauri/src/routing/engine.rs
Зависимости: TASK-002, TASK-004
Критерий готовности: тестовый сценарий "Arma3 → Headphones" проигрывает звук;
  mute/volume работают; splitter разделяет поток на 2 выхода
Тесты: unit тесты volume/mute применения к буферу; интеграционный тест (мануал)

### TASK-006: Virtual devices — Virtual Mic + Virtual Output
Описание: Создать виртуальные аудио устройства Nodus через Windows Audio APIs
  (WASAPI + VB-Audio Cable как первый шаг ИЛИ использование существующих
  virtual loopback endpoints). Динамическое создание/удаление 3+ устройств.
  
  Реалистичная оценка: настоящий виртуальный драйвер требует kernel driver.
  MVP реализует через:
  1. Проверку наличия VB-Audio Virtual Cable / Virtual Audio Cable
  2. Если есть — роутинг через них
  3. Если нет — инструкция пользователю + graceful fallback
  
  Виртуальные устройства появляются в системе как аудио устройства и
  доступны для Discord, OBS и других приложений.
Файлы:
  - src-tauri/src/audio/virtual_device.rs
Зависимости: TASK-002, TASK-005
Критерий готовности: Discord видит Nodus Virtual Mic как вход; OBS видит
  Nodus Virtual Output как аудио устройство; 3+ виртуальных устройства доступны
Тесты: unit тесты создания конфигурации; интеграционный тест (мануал)

### TASK-007: Tauri commands bridge — все invoke команды
Описание: Зарегистрировать все Tauri команды из контракта в CLAUDE.md:
  get_audio_devices, apply_routing_graph, set_route_mute, set_route_volume,
  get_running_audio_processes. Подключить события Rust → UI.
Файлы:
  - src-tauri/src/commands/bridge.rs
  - src-tauri/src/commands/mod.rs
  - src-tauri/src/main.rs (register commands)
Зависимости: TASK-002, TASK-003, TASK-004, TASK-005
Критерий готовности: все команды зарегистрированы; события emit_all работают;
  тесты сериализации входных/выходных типов проходят
Тесты: unit тесты сериализации AudioDevice, RoutingGraph, AudioProcess

---

## Последовательность выполнения

```
TASK-001 (scaffolding)
    ├─→ TASK-002 (WASAPI devices)  ─────────────────┐
    ├─→ TASK-003 (process detect)                    │
    └─→ TASK-004 (routing graph)  ──→ TASK-005 ──→ TASK-006
                                        (engine)    (virtual)
                                                        │
                                                    TASK-007 (bridge)
```

Параллельно: 002 + 003 + 004 (все зависят только от 001).
Параллельно: 005 + частично 006 (после 002 + 004).
