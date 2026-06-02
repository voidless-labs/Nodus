# TASK-006 — Virtual Devices

## Что сделано

`virtual_device.rs` реализован полностью в рамках возможного без kernel driver:

- `VIRTUAL_DRIVER_MARKERS` — список маркеров имён: "CABLE", "VoiceMeeter", "Virtual Audio", "Nodus"
- `find_virtual_devices(all_devices)` — фильтрует список устройств по маркерам, тегирует как DeviceType::Virtual
- `query_virtual_status(all_devices)` — возвращает статус + сообщение для UI
- `VirtualDevice` struct — описывает виртуальное устройство с render_device_id + capture_device_id

## Результат проверки на реальной машине

VB-Audio Virtual Cable не установлен. Нет виртуальных устройств.
`query_virtual_status` возвращает:
```
available: false
message: "No virtual audio devices found. Install VB-Audio Virtual Cable (free)..."
```

## Архитектурное решение

**Проблема**: настоящий Virtual Mic/Output требует kernel-mode audio driver.
Nodus не может создавать виртуальные устройства без установленного драйвера.

**MVP решение**:
1. Пользователь устанавливает VB-Audio Virtual Cable (бесплатно)
2. Nodus обнаруживает устройства по имени
3. Routing через VB-Audio: приложение → VB-Audio Input → Nodus capture → Nodus render → VB-Audio Output → Discord

**Команда для UI**: `get_audio_devices()` уже возвращает VB-Audio устройства как `DeviceType::Virtual`.
UI показывает их как Virtual Mic / Virtual Output ноды.

**Если VB-Audio не установлен**: `get_audio_devices()` возвращает только реальные устройства.
UI должен показать подсказку (эта логика в UI, не в Rust).

## Почему "динамическое создание" невозможно без драйвера

Создание нового аудио устройства в Windows = установка kernel driver.
Это требует:
- Signed kernel driver (CODE SIGNING CERTIFICATE ~$300/год)
- Windows Driver Framework
- Administrator rights + UAC elevation
- Перезагрузку системы

Для MVP: VB-Audio Cable = правильный подход. Он бесплатный, подписанный, WHQL-certified.

## Тесты

- `vb_audio_detected_as_virtual` ✅
- `find_virtual_devices_filters_correctly` ✅  
- `status_message_when_no_virtual_drivers` ✅

## Известные ограничения

- "Динамическое создание 3+ устройств" из MVP требует VB-Audio + UI
- VB-Audio Cable даёт 1 виртуальный пар (Input+Output). Для 3+ — нужен VB-Audio Hi-Fi Cable или несколько копий
- Nodus-branded Virtual Devices (долгосрочно) — требует собственного WDF driver (Phase 2+)

## Следующий шаг

TASK-007 — финальная проверка Tauri bridge.
Все команды реализованы. Ждём UI для интеграции.
