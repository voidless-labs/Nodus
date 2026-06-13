# t5 — Несколько устройств, создание из UI

**Статус:** ⏸ ждёт t2 и t3 (механика одного устройства каждого типа)
**Кто делает:** я

## Суть простыми словами
Пользователь в Nodus нажимает «создать виртуальное устройство», даёт имя
(например «OBS Music») — и оно появляется в Windows. Удалил — исчезло.
Это пункт MVP «динамическое создание/удаление виртуальных устройств (3+)».

## Критерий готовности
- [ ] Из UI можно создать 3+ устройств (вход/выход) со своими именами — без UAC-запроса
- [ ] Устройства переживают перезагрузку (восстанавливаются сами)
- [ ] Удаление убирает устройство из Windows и освобождает ресурсы
- [ ] У каждого устройства своё кольцо — звук не перемешивается

## Полевой smoke шага 2 (13.06.2026) — нашёл архитектурный изъян control-канала
- Виртуальный микрофон ВИДЕН в системе («Микрофон (Nodus Virtual Audio)») — t3 крит.1 ✅.
- `device-ctl.exe`: интерфейс НАЙДЕН флагом PRESENT (значит IoRegisterDeviceInterface +
  IoSetDeviceInterfaceState(TRUE) отработали), но `CreateFileW` → 0x80070002 FILE_NOT_FOUND.
- Диагноз: IOCTL повешены на FDO PortCls, но открытие хэндла (IRP_MJ_CREATE) обрабатывает
  KS/PortCls и отвергает открытие нашего кастомного интерфейса. Перехвачены DEVICE_CONTROL
  и PNP, но НЕ CREATE/CLOSE. ADR §3.1 выбрал «без отдельного устройства» ради простоты —
  поле показало, что так хэндл не открыть.
- РЕШЕНИЕ (шаг 2b, перед шагом 3): отдельное control-устройство `IoCreateDeviceSecure`
  со своим SDDL (доступ обычному пользователю) + symlink/интерфейс на НЁМ; наш create/
  close/devicecontrol на этом device object, KS не мешает. ADR обновить (вернуть
  отвергнутую альтернативу с обоснованием от поля).
- ✅ СДЕЛАНО 13.06.2026 (step 2b): `\Device\NodusControl` + symlink
  `\\.\NodusControl`, SDDL SYSTEM/Admins=all, World=RW; перехват CREATE/CLOSE/
  DEVICE_CONTROL/PNP с маршрутизацией по DeviceObject; создание в DriverEntry,
  удаление в DriverUnload. Rust открывает `\\.\NodusControl` напрямую (CfgMgr32
  убран). ADR §3 обновлён. cargo test 65/65, собрано локально (Rust) — kernel ждёт CI.
  Полевой smoke: `device-ctl.exe` → версия 1 + 2 статических устройства.

## Технические заметки (для меня)
- Путь выбран (см. разбор в чате 11.06): один devnode, динамические subdevice.
  Драйвер по IOCTL создаёт/удаляет пары wave+topo в рантайме:
  `PcRegisterSubdevice` после старта + `IUnregisterSubdevice`/`IUnregisterPhysicalConnection`.
  Живой референс — bluetooth-sideband endpoints в SYSVAD (`bthhfpdevice.cpp`).
- Control-интерфейс с ACL на обычного пользователя: CREATE / REMOVE / LIST.
- Кольца уже параметрические с t2 (`Global\NodusRing-<id>`).
- Персистентность: список устройств в реестре под ключом устройства, драйвер
  восстанавливает при старте системы.
- Rust: Tauri-команды `create_virtual_device` / `remove_virtual_device`,
  DeviceIoControl через windows crate, событие в UI об изменении списка.
- НЕ путь «devnode на устройство»: каждый запрос требовал бы прав администратора.
