# r4 — t5 шаги 1–2: Rust-зеркало контракта + device-ctl

Выжимка: Rust получил зеркало IOCTL-контракта (`device_control.rs`:
repr(C)-структуры, коды, GUID, обнаружение интерфейса через CfgMgr32,
обёртки query_version/list_devices) и диагностическую утилиту `device-ctl.exe`,
которая на ноуте покажет версию протокола и таблицу устройств. Код — агент
backend; верификация — Team Lead (агент оборвался на лимите): `cargo build`
чисто, `cargo test` зелёный, включая 12 тестов device_control (оффсеты,
коды IOCTL, GUID, кодирование имени).

## Файлы
Новые:
- `src/audio/device_control.rs` — зеркало `nodus_ioctl.h`: коды IOCTL литералами
  с проверкой против ADR, repr(C)-структуры с offset-тестами, GUID-константа с
  тестом соответствия, open_control() через CM_Get_Device_Interface_ListW +
  CreateFileW, query_version()/list_devices() через DeviceIoControl, заглушки
  create/destroy (kernel пока отвечает NOT_IMPLEMENTED). cfg(windows) +
  не-Windows заглушка.
- `src/bin/device_ctl.rs` — CLI: найти интерфейс → QUERY_VERSION → LIST_DEVICES →
  печать таблицы; различает «драйвер не установлен» / «старый драйвер без
  канала» / «канал есть, IOCTL не отвечает».

Изменённые:
- `src/audio/mod.rs` — регистрация device_control.
- `Cargo.toml` — bin device-ctl; features windows: DeviceAndDriverInstallation,
  Storage_FileSystem, System_IO.

## Верификация (Team Lead)
- `cargo build` — чисто.
- `cargo test` — зелёный; 12 device_control-тестов (query/create/destroy/list
  layout, ioctl_codes_match_adr, protocol_constants_match_adr,
  control_guid_matches_team_lead_issue, friendly_name_encode_decode_roundtrip,
  kind_wire_roundtrip).
- `device-ctl.exe` собран в release (static CRT).
- Контракт сходится с kernel-стороной по C_ASSERT/offset_of с двух сторон;
  GUID 56AEE59C-… идентичен выданному Team Lead и kernel-DEFINE_GUID.
- Рантайм с реальным драйвером — только в поле (ноут недоступен).

## Полевой smoke (вместе с t3/t4)
Установить t5-сборку → `device-ctl.exe` → «Protocol version 1, max dynamic 8» +
2 статических устройства. Это закрывает критерий шага 2 из ADR §13.
