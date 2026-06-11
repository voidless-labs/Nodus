# Nodus Virtual Audio Driver — миграция на SYSVAD

Решение (03.06.2026): самописный WaveRT-драйвер признан тупиковым (2 подтверждённых
BSOD на базовых ошибках, нет топологии → endpoint не появляется, нет capture,
секция без security descriptor, баги IRQL/DPC). Переходим на архитектуру Microsoft
**SYSVAD** (audio/sysvad из windows-driver-samples, MIT) — известно-рабочая база с
render+capture endpoints, топологией и корректным lifecycle.

## Цель
Один PortCls-драйвер, два виртуальных устройства, оба видны в Windows Sound Settings:
- **Nodus Virtual Speaker** (render): приложения играют → драйвер копирует PCM →
  render-ring → Nodus читает (`VirtualCapture`).
- **Nodus Virtual Mic** (capture): Nodus пишет → capture-ring → драйвер отдаёт как
  данные микрофона → приложения (Discord) читают.

## Архитектура (паттерн SYSVAD, на каждый endpoint)
- **Wave-миniport (WaveRT) + Topology-миniport**, зарегистрированы как два subdevice,
  связаны `PcRegisterPhysicalConnection`.
- Wave-фильтр: host-пин (со стороны приложения) + bridge-пин (к топологии).
- Topology-фильтр: bridge-пин + connector-пин с категорией `KSNODETYPE_SPEAKER`
  (render) / `KSNODETYPE_MICROPHONE` (capture) — без этого MMDevAPI не строит endpoint.

## IPC (наша часть поверх SYSVAD)
- `common.h`: ДВА кольца — render (speaker→Nodus) и capture (Nodus→mic). Каждое:
  header + 2с f32 stereo 48k ring, монотонные WriteBytes/ReadBytes.
- Именованные секции `Global\NodusRenderRing` / `Global\NodusCaptureRing` с **явным
  security descriptor** (RW для Authenticated Users), чтобы Nodus (обычный пользователь)
  мог OpenFileMappingW.
- Ядро мапит каждое кольцо в **system space** (`MmMapViewInSystemSpace`) — доступ из
  timer-DPC на DISPATCH_LEVEL безопасен (в отличие от текущего user-context маппинга).
- Render-stream timer: WaveRT cyclic buffer → render-ring. Capture-stream timer:
  capture-ring → WaveRT buffer, отдаваемый приложению.

## Фазы
1. **Фундамент.** Вендорим SYSVAD (или минимальный SYSVAD-паттерн скелет), собираем и
   подписываем в существующем CI (EWDK). Критерий: «Nodus Virtual Speaker» появляется
   в Sound Settings (пока без IPC — тишина/discard), без BSOD.
2. **Capture endpoint.** «Nodus Virtual Mic» появляется как устройство записи.
3. **IPC.** Защищённые двойные кольца, system-space маппинг; render-stream → render-ring;
   capture-stream ← capture-ring.
4. **Связка с Nodus.** `VirtualCapture` читает render-ring; новый `VirtualRender` пишет
   capture-ring; движок маршрутизирует.
5. **Hardening.** Отмена/flush DPC при teardown стрима, согласование формата,
   дизайн multi-instance (3+ устройств = секции с уникальными именами).
6. **Attestation-подпись** через Partner Center — запускать аккаунт/EV-сертификат СЕЙЧАС
   (параллельно, долгий лид-тайм). Без неё драйвер не ставится у пользователей без Test Mode.

## Что НЕ трогаем
Rust-движок, routing, process-loopback, UI, WASAPI-loopback fallback — развязаны с драйвером.

## Замечания
- Текущий `main` содержит старый драйвер с известным BSOD (фикс деструктора `e3764ae`
  остался в ветке `feat/...`, в main не влит). При миграции старый драйвер заменяется,
  поэтому отдельно его не чиним (если только не нужен временный тест).
- Цикл CI ~20 мин; тестируем на расходном Win10-ноуте, WinDbg-флоу символизации отлажен.
