# TASK-002/003/004/005 — WASAPI + Routing Integration

## Что сделано

Проведён интеграционный тест на реальном железе через `cargo run --bin nodus-check`.

### Результаты теста (30.05.2026)

**Аудио устройства (6 шт.):**
- Galaxy Buds FE (Output)
- Динамики MIXLINE (Output, default)  
- Динамики Realtek High Definition Audio (Output)
- Микрофон Fifine (Input, default)
- Микрофон MIXLINE Record (Input)
- Микрофон MIXLINE Stream (Input)

**Аудио процессы (5 уникальных):**
- Discord (Chat)
- Edge (Browser)
- Firefox (Browser)
- Spotify (Music)
- Steam (System)

**Routing engine:**
- Успешно запустил loopback capture на Galaxy Buds FE
- Успешно запустил render на MIXLINE
- set_route_mute() работает в реальном времени
- Engine start/stop без паник

## Исправления в этой сессии

**Дедупликация процессов**: Firefox, Discord, Spotify запускают много дочерних процессов
с одним exe-именем. Исправлено через HashMap по lowercase exe-имени — один процесс
на exe (первый найденный PID). Результат отсортирован по display_name.

**LoopbackCapture::start()**: исправлен на idempotent — повторный вызов возвращает
новый subscriber вместо запуска второго потока захвата.

**lib.rs**: добавлен для доступа из интеграционных бинарников (nodus-check).

## Созданные файлы

- `src-tauri/src/lib.rs` — экспортирует все модули
- `src-tauri/src/bin/nodus_check.rs` — интеграционный тест (запуск: cargo run --bin nodus-check)

## Тесты

```
cargo test --lib — 30 passed; 0 failed
cargo run --bin nodus-check — ✅ all checks passed
```

## Известные ограничения

- Routing передаёт звук через loopback → render, но audible тест (прослушать звук)
  требует VB-Audio Cable для Source → Virtual Mic сценария
- MIXLINE — это внешнее устройство (аудиокарта), не Nodus Virtual

## Следующий шаг

TASK-006 — Virtual devices: проверить наличие VB-Audio/Virtual Cable,
настроить роутинг через них.

TASK-007 — Tauri bridge финальная проверка (bridge.rs готов, нужен UI).
