# r3 — t5 шаги 1–2: kernel-сторона контрольного канала IOCTL

Выжимка: в драйвер добавлен контрольный IOCTL-канал — заголовок-контракт
`nodus_ioctl.h` (коды, GUID, структуры с C_ASSERT-оффсетами) и реализация
`nodus_control.cpp/.h` (перехват IRP_MJ_DEVICE_CONTROL/PNP поверх PcDispatchIrp,
регистрация device interface, обработчики QUERY_VERSION и LIST_DEVICES — пока
отдаёт две статические записи; CREATE/DESTROY валидируют вход и возвращают
STATUS_NOT_IMPLEMENTED). Код написан агентом driver; **связки в DriverEntry/
AddDevice и запись в vcxproj дописаны Team Lead** (агент оборвался на лимите) —
ревью APPROVE.

## Файлы
Новые:
- `nodus_ioctl.h` — контракт control-канала: GUID {56AEE59C-…}, 4 IOCTL-кода
  (FILE_DEVICE_UNKNOWN/METHOD_BUFFERED/FILE_ANY_ACCESS) с пин-значениями
  (0x222000/4/8/C), структуры запросов/ответов pack(8) с C_ASSERT на каждый
  оффсет и размер. Чистый C — включается и в kernel, и в device-ctl.exe.
- `nodus_control.cpp/.h` — NODUS_ADAPTER_CONTEXT (PDO/FDO/symlink/Removing/
  KMUTEX); NodusControlInit/OnAddDevice/EnableInterface; диспетчеры
  NodusDispatchDeviceControl (развязка KS↔наши по DEVICE_TYPE_FROM_CTL_CODE) и
  NodusDispatchPnp (teardown под мьютексом на REMOVE/SURPRISE_REMOVAL до
  PcDispatchIrp); единственная точка завершения IRP; защита по IRQL; полная
  валидация входа до обработки.

Изменённые:
- `adapter.cpp` — include nodus_control.h (под INITGUID — эмитит storage GUID);
  StartDevice → NodusControlEnableInterface (некритично); **AddDevice →
  NodusControlOnAddDevice после успешного PcAddAdapterDevice; DriverEntry →
  NodusControlInit + перехват MajorFunction[DEVICE_CONTROL]/[PNP] после
  PcInitializeAdapterDriver** (эти связки — правка Team Lead).
- `nodus_audio.vcxproj` — nodus_control.cpp в ClCompile, nodus_ioctl.h/
  nodus_control.h в ClInclude (тоже Team Lead — без этого CI не собрал бы).

## Найдено и исправлено при ревью
Агент не успел: (1) подключить диспетчеры в DriverEntry, (2) захват PDO в
AddDevice, (3) добавить .cpp/.h в vcxproj. Без любого из трёх канал был бы
мёртвым кодом (или вовсе не скомпилировался). Дописано по сигнатурам из
nodus_control.h (там в комментариях явно расписано, что куда). Render/capture-
путь (t1–t4) не затронут.

## Отдаётся шагу 3
CREATE/DESTROY: аллокация id, InstallSubdevice ×2 с Irp=NULL,
PcRegisterPhysicalConnection, FriendlyName в ключ интерфейса, запись в таблицу
устройств (место под неё зарезервировано в NODUS_ADAPTER_CONTEXT), teardown в
NodusDispatchPnp. Всё под уже существующим мьютексом.

## Smoke-тест в поле (шаг 2)
После установки t5-сборки: `device-ctl.exe` → должен найти интерфейс, напечатать
«Protocol: version 1 … max dynamic 8» и таблицу из 2 статических устройств
(id 0 render «Nodus Virtual Speaker», id 0 capture «Nodus Virtual Mic»).
Главное — звук статики (t1–t4) не деградировал. Проверка только в поле (CI
подтверждает лишь компиляцию).

## Верификация
Локальной сборки нет (EWDK в CI). Статически: контракт самосогласован
(C_ASSERT), IRP всегда завершается ровно один раз, чужой трафик уходит в
PcDispatchIrp без утечек, всё PASSIVE. Решающая проверка компиляции — CI.
