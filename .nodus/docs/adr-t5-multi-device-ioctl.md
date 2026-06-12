# ADR: t5 — протокол динамического создания/удаления виртуальных аудиоустройств

**Дата:** 13.06.2026
**Статус:** принят (детализация решения от 11.06: один devnode + динамические subdevice-пары)
**Автор:** architect
**Затрагивает:** `src-tauri/driver/nodus_audio/*`, `src-tauri/src/audio/*`, `src-tauri/src/commands/bridge.rs`, `nodus_audio.inf`

---

## 1. Контекст

Пользователь в Nodus нажимает «создать виртуальное устройство», задаёт имя — устройство
появляется в Windows без UAC. Удалил — исчезло. До 8 динамических устройств, каждое со
своим кольцом.

Базовое решение принято ранее и не пересматривается: **один devnode
`ROOT\NodusVirtualAudio`, динамические пары субдевайсов (wave + topology) через IOCTL**,
регистрация `PcRegisterSubdevice` в рантайме, снятие через `IUnregisterSubdevice`.
Референс — bluetooth-sideband endpoints в Microsoft SYSVAD (`bthhfpdevice.cpp` /
`CAdapterCommon::InstallSubdevice` с `Irp = NULL` вне StartDevice). Путь
«devnode на устройство» отвергнут: каждое создание devnode требует elevation.

Что уже есть (t1–t4, проверено в поле):

- StartDevice статически ставит 4 субдевайса: `Wave`/`Topology` (render, «Nodus Virtual
  Speaker») и `WaveCap`/`TopologyCap` (capture, «Nodus Virtual Mic»).
- Имена колец уже параметрические: `\BaseNamedObjects\NodusRing-%u` (render, Everyone
  read) и `\BaseNamedObjects\NodusRing-mic-%u` (capture, Everyone read+write). Статика
  использует id = 0.
- Ленивое создание секций в NewStream (EnsureRing) — на холодной загрузке
  `\BaseNamedObjects` ещё не существует.
- Rust: `ring_layout.rs` (зеркало layout'а с offset-тестами), `virtual_capture.rs` /
  `virtual_render.rs` работают с id = 0.

### Незыблемые kernel-правила проекта, которые протокол обязан соблюдать

(из комментариев `ring.cpp`, `minwavert.h`, `minwavertstream.cpp` и `.nodus/task/done.md`)

| Правило | Как протокол его учитывает |
|---|---|
| Секции создаются лениво (boot: нет `\BaseNamedObjects`) | EnsureRing остаётся единственной точкой создания кольца, параметризуется id; IOCTL кольцо НЕ создаёт |
| Копировальный поток join'ится ДО освобождения буфера | DESTROY не трогает стримы напрямую — teardown идёт через существующую цепочку refcount'ов |
| Миниport владеет кольцом, стримы заимствуют view | Кольцо умирает в деструкторе миниporta, а не в обработчике DESTROY |
| Single-writer arbitration (ClaimWriter) | Per-miniport, автоматически работает для каждой динамической пары |
| Явный DACL секций (SYSTEM/Admins full, Everyone read[+write]) | Без изменений, выбор WorldWritable по kind |
| Всё про кольца — только PASSIVE_LEVEL | Все IOCTL-обработчики PASSIVE, METHOD_BUFFERED |
| Обновление драйвера = перезапуск устройства (старый .sys в памяти) | Динамика умирает с devnode; восстановление — тот же путь, что после ребута (см. §7) |
| .ps1 — только ASCII | Касается правок install.ps1 (если будут) |

---

## 2. Решение (сводка)

1. **Контрольный канал** — хук `IRP_MJ_DEVICE_CONTROL` поверх `PcDispatchIrp` на
   существующем FDO + собственный device interface GUID для обнаружения из userspace.
   Отдельного control-device НЕТ.
2. **4 IOCTL**: QUERY_VERSION, CREATE_DEVICE, DESTROY_DEVICE, LIST_DEVICES.
   `FILE_DEVICE_UNKNOWN`, `METHOD_BUFFERED`, `FILE_ANY_ACCESS` — обычный пользователь
   создаёт устройства без elevation.
3. **CREATE создаёт один endpoint** (render ИЛИ capture) = пара субдевайсов
   `Wave-<id>`/`Topology-<id>` (или `WaveCap-<id>`/`TopologyCap-<id>`) + физсвязь.
   Кольцо — лениво, по существующему пути.
4. **FriendlyName**: основной путь — драйвер пишет значение `FriendlyName` в registry-ключ
   device interface из kernel (минуя ACL userspace); fallback — преднастроенные в INF
   имена-слоты «Nodus Virtual Device N».
5. **Лимит 8 динамических** устройств, id 1..8 (0 зарезервирован за статикой),
   единое пространство id для обоих kind'ов, CREATE умеет запрашивать конкретный id
   (нужно для восстановления после ребута).
6. **Персистентность — на стороне Nodus** (конфиг приложения + повторные CREATE при
   старте). Драйвер ничего не хранит.
7. **Миграция**: статическая пара остаётся «устройством 0» и не удаляется через протокол.

### 2.1 Продуктовая политика (уточнения пользователя, 13.06.2026)

Решения владельца продукта, наложенные поверх протокола (сам протокол остаётся
универсальным — политика живёт в приложении):

- **Колонка — ровно одна** (статическая, «устройство 0»): её назначение — перехват
  всего системного звука; разделение по приложениям делает движок (process loopback),
  поэтому UI создание render-устройств НЕ предлагает. CREATE(kind=render) в протоколе
  сохраняется (тесты, будущее), но пользователю не виден.
- **Микрофоны — плодятся**: базовый «Nodus Virtual Mic» (статический) + динамические.
  Имя пользователя передаётся как есть; если имя не задано — приложение само
  генерирует «Nodus Virtual Mic #2, #3…» (драйвер нумерацию не знает).
- **Лимит 8 динамических** подтверждён.
- **Переименование колонки**: устройство переименовать так, чтобы в Windows
  отображалось «Динамики (Nodus)» (DeviceDesc/Strings в INF) — выполнить вместе с
  шагом 4 (правки INF там и так есть).
- **Вне протокола** (отдельная задача t7, backend+ui-bridge): если вывод по умолчанию
  в Windows ≠ наша колонка — Nodus показывает уведомление «не могу перехватить звук
  системы» с инструкцией.
- Персистентность (§8, восстанавливает Nodus) и доступ без elevation (§4.2) —
  подтверждены пользователем.

---

## 3. Контрольный канал

### 3.1 Как встроить IOCTL в PortCls-драйвер

`PcInitializeAdapterDriver` забирает все dispatch-точки под `PcDispatchIrp`
(через него идут все KS property-вызовы как `IOCTL_KS_*`). Стандартный приём —
тот же, что в SYSVAD для PnP: **после** `PcInitializeAdapterDriver` в DriverEntry
перехватываем две точки и chain'имся:

```cpp
// DriverEntry, после PcInitializeAdapterDriver:
DriverObject->MajorFunction[IRP_MJ_DEVICE_CONTROL] = NodusDispatchDeviceControl;
DriverObject->MajorFunction[IRP_MJ_PNP]            = NodusDispatchPnp;
```

`NodusDispatchDeviceControl`:

```cpp
ULONG code = IrpSp->Parameters.DeviceIoControl.IoControlCode;
if (DEVICE_TYPE_FROM_CTL_CODE(code) == FILE_DEVICE_UNKNOWN &&
    code >= IOCTL_NODUS_QUERY_VERSION && code <= IOCTL_NODUS_LIST_DEVICES)
    return NodusHandleControl(DeviceObject, Irp);   // наш
return PcDispatchIrp(DeviceObject, Irp);            // всё остальное — PortCls
```

Разводка однозначна: KS использует `FILE_DEVICE_KS` (0x2F), мы — `FILE_DEVICE_UNKNOWN`
(0x22); тип устройства зашит в биты 16–31 IOCTL-кода. Хук на PNP нужен для
`IRP_MN_REMOVE_DEVICE` / `IRP_MN_SURPRISE_REMOVAL`: освободить ссылки на динамические
порты и погасить control-интерфейс ДО передачи IRP в `PcDispatchIrp` (см. §9).

**Почему не отдельный control-device** (`IoCreateDeviceSecure`): второй device object —
это второй жизненный цикл (создание/удаление синхронно с PnP-состоянием FDO), свой
symlink, свой обработчик create/close. Хук — одна функция, прецедент в SYSVAD, нулевая
дополнительная поверхность. Единственный плюс отдельного устройства — собственный SDDL —
нам не нужен (см. §4.2).

### 3.2 Обнаружение из userspace

В `AddDevice` сохраняем PDO (драйвер односекционный по архитектуре — глобальный
`NODUS_ADAPTER_CONTEXT` допустим; повторный AddDevice → второй devnode работает без
control-канала, логируем). В `StartDevice`:

```cpp
IoRegisterDeviceInterface(g_Adapter.Pdo, &GUID_DEVINTERFACE_NODUS_CONTROL,
                          nullptr, &g_Adapter.ControlSymlink);
IoSetDeviceInterfaceState(&g_Adapter.ControlSymlink, TRUE);
```

GUID генерируется один раз и фиксируется в `nodus_ioctl.h` (новый общий заголовок,
зеркалится в Rust): `GUID_DEVINTERFACE_NODUS_CONTROL`.

Rust находит путь через CfgMgr32 (легче SetupDi, есть в `windows` crate):
`CM_Get_Device_Interface_List_SizeW` + `CM_Get_Device_Interface_ListW
(CM_GET_DEVICE_INTERFACE_LIST_PRESENT)` → `CreateFileW(path, GENERIC_READ |
GENERIC_WRITE, FILE_SHARE_READ | FILE_SHARE_WRITE, ...)`.

Открытие без admin работает: device object создан PortCls в классе MEDIA, аудиоустройства
по определению world-openable (иначе приложения не открывали бы пины).

---

## 4. Набор IOCTL

### 4.1 Коды

```c
// nodus_ioctl.h — единый контракт kernel <-> userspace (как common.h для колец)
#define NODUS_CTL_VERSION 1u

#define IOCTL_NODUS_QUERY_VERSION  CTL_CODE(FILE_DEVICE_UNKNOWN, 0x800, METHOD_BUFFERED, FILE_ANY_ACCESS)
#define IOCTL_NODUS_CREATE_DEVICE  CTL_CODE(FILE_DEVICE_UNKNOWN, 0x801, METHOD_BUFFERED, FILE_ANY_ACCESS)
#define IOCTL_NODUS_DESTROY_DEVICE CTL_CODE(FILE_DEVICE_UNKNOWN, 0x802, METHOD_BUFFERED, FILE_ANY_ACCESS)
#define IOCTL_NODUS_LIST_DEVICES   CTL_CODE(FILE_DEVICE_UNKNOWN, 0x803, METHOD_BUFFERED, FILE_ANY_ACCESS)
```

### 4.2 Почему FILE_ANY_ACCESS — обоснование

Модель Nodus: приложение работает БЕЗ elevation и должно создавать устройства. Проверка
доступа DeviceIoControl идёт по правам handle (CreateFile) против access-битов IOCTL.
`FILE_ANY_ACCESS` пропускает любой успешно открытый handle, а открыть MEDIA-устройство
может любой локальный пользователь — ровно то, что нужно. Это сознательно означает: любой
локальный процесс может создавать/удалять виртуальные устройства. Принимаем для
single-user desktop, потому что: (а) ресурс жёстко ограничен (8 устройств, фиксированные
аллокации, всё валидируется); (б) худший исход злоупотребления — пропали/появились
аудиоустройства, не повреждение системы. Ужесточение (проверка integrity level через
`SeQuerySubjectContextEx` в обработчике) — задел на Phase 2, в протокол не входит.
SDDL на device interface не помогает: symlink указывает на тот же device object, который
обязан оставаться world-openable для аудио.

### 4.3 Версионирование

Двухуровневое, по образцу кольца (Magic/Version в заголовке):

- `IOCTL_NODUS_QUERY_VERSION` возвращает `NODUS_CTL_VERSION` — Nodus проверяет ПЕРВЫМ
  вызовом и отказывается работать с незнакомой мажорной версией (UI: «обновите драйвер»).
- Каждая входная/выходная структура начинается с `ULONG Size`; драйвер требует точного
  совпадения с `sizeof` своей версии структуры. Совместимые расширения занимают
  Reserved-поля без смены Size; несовместимые — бамп `NODUS_CTL_VERSION` и Size.

---

## 5. Структуры запросов/ответов

Все — `#pragma pack(push, 8)`, фиксированные оффсеты, C_ASSERT'ы в драйвере +
offset-тесты в Rust (`#[repr(C)]`, паттерн `ring_layout.rs`). Все поля выровнены
натурально, поэтому plain `repr(C)` воспроизводит layout без сюрпризов.

```c
#define NODUS_MAX_DYNAMIC_DEVICES 8u
#define NODUS_MAX_NAME_CCH        64u   // WCHAR, включая NUL
#define NODUS_TOTAL_DEVICE_SLOTS  10u   // 2 статических + 8 динамических

// Kind (ULONG на проводе)
#define NODUS_KIND_RENDER  0u   // виртуальная колонка: кольцо NodusRing-<id>
#define NODUS_KIND_CAPTURE 1u   // виртуальный микрофон: кольцо NodusRing-mic-<id>

// Flags в NODUS_DEVICE_INFO
#define NODUS_DEVFLAG_STATIC      0x1u  // статическая пара, DESTROY запрещён
#define NODUS_DEVFLAG_RING_ACTIVE 0x2u  // секция кольца создана (EnsureRing сработал)
```

`NODUS_QUERY_VERSION_OUTPUT` — 16 байт:

| Оффсет | Поле | Тип | Значение |
|---|---|---|---|
| 0 | Size | ULONG | 16 |
| 4 | Protocol | ULONG | NODUS_CTL_VERSION |
| 8 | MaxDynamicDevices | ULONG | 8 |
| 12 | Reserved0 | ULONG | 0 |

`NODUS_CREATE_DEVICE_INPUT` — 152 байта:

| Оффсет | Поле | Тип | Семантика |
|---|---|---|---|
| 0 | Size | ULONG | 152 |
| 4 | Kind | ULONG | NODUS_KIND_RENDER / _CAPTURE |
| 8 | RequestedId | ULONG | 0 = авто (минимальный свободный), 1..8 = явный (для восстановления после ребута, §7) |
| 12 | Flags | ULONG | 0, резерв |
| 16 | FriendlyName | WCHAR[64] | UTF-16, NUL-терминированное, непустое |
| 144 | Reserved0 | ULONGLONG | 0 |

`NODUS_CREATE_DEVICE_OUTPUT` — 16 байт:

| Оффсет | Поле | Тип | Семантика |
|---|---|---|---|
| 0 | Size | ULONG | 16 |
| 4 | Id | ULONG | назначенный id (1..8); он же id кольца |
| 8 | Reserved0 | ULONGLONG | 0 |

`NODUS_DESTROY_DEVICE_INPUT` — 16 байт: `{ ULONG Size=16; ULONG Id; ULONG Flags=0; ULONG Reserved0; }`

`NODUS_DEVICE_INFO` — 144 байта:

| Оффсет | Поле | Тип |
|---|---|---|
| 0 | Id | ULONG |
| 4 | Kind | ULONG |
| 8 | Flags | ULONG (STATIC / RING_ACTIVE) |
| 12 | Reserved0 | ULONG |
| 16 | FriendlyName | WCHAR[64] |

`NODUS_LIST_DEVICES_OUTPUT` — 16 + 10×144 = 1456 байт (фиксированный, снимок):

| Оффсет | Поле | Тип |
|---|---|---|
| 0 | Size | ULONG = 1456 |
| 4 | Count | ULONG — заполненных записей |
| 8 | MaxDynamicDevices | ULONG = 8 |
| 12 | Reserved0 | ULONG |
| 16 | Devices | NODUS_DEVICE_INFO[10], валидны первые Count |

Валидация в драйвере (METHOD_BUFFERED, всё из SystemBuffer): длины буферов ≥ Size,
Size == sizeof, Kind ≤ 1, RequestedId ≤ 8, имя — NUL найден в пределах 64 WCHAR, не
пустое. Любое нарушение → `STATUS_INVALID_PARAMETER`, ничего не аллоцируется.

Коды ошибок: лимит исчерпан → `STATUS_QUOTA_EXCEEDED`; RequestedId занят →
`STATUS_OBJECT_NAME_COLLISION`; DESTROY несуществующего → `STATUS_NOT_FOUND`;
DESTROY id 0 → `STATUS_INVALID_PARAMETER`; устройство в PnP-remove →
`STATUS_DEVICE_NOT_READY`. Rust мапит в человекочитаемые `Err(String)`.

---

## 6. Семантика CREATE / DESTROY

### 6.1 CREATE — что физически происходит

Один CREATE = один endpoint = **два** субдевайса (не четыре: четыре — это две статические
пары t1+t3). Под глобальным мутексом, PASSIVE_LEVEL:

1. Валидация входа, выбор id (RequestedId или минимальный свободный из 1..8).
2. Reference-имена: render → `Wave-%u` / `Topology-%u`; capture → `WaveCap-%u` /
   `TopologyCap-%u` (swprintf, id в десятичной форме).
3. Существующий `InstallSubdevice` из adapter.cpp, с двумя изменениями:
   - вызывается с `Irp = NULL` (порт `Init` принимает NULL вне StartDevice — паттерн
     SYSVAD sideband) и пустым resource list (`PcNewResourceList`, миниporty его
     игнорируют);
   - фабрики миниportov получают параметр `ULONG RingId` (статический путь передаёт 0),
     EnsureRing подставляет его в существующие шаблоны имён колец.
4. Запись FriendlyName в registry интерфейсов (см. §6.2) — ДО физсвязи, чтобы
   AudioEndpointBuilder увидел имя при построении endpoint'а.
5. `PcRegisterPhysicalConnection`: render — wave(bridge out) → topo(bridge in);
   capture — topo(bridge out) → wave(bridge in). Те же пины, что в статике.
6. Запись в таблицу адаптера: `{ Id, Kind, Name, PPORT Wave, PPORT Topo }` —
   **ссылки на порты удерживаются** (в отличие от статики, где они отпускаются после
   регистрации: динамике они нужны для IUnregisterSubdevice).
7. Откат при любой ошибке: снять то, что успело зарегистрироваться, отпустить порты,
   слот освободить.

Кольцо в CREATE **не создаётся**: остаётся ленивый EnsureRing в NewStream. Для
IOCTL-создаваемых устройств `\BaseNamedObjects` уже существует, но единый проверенный
путь проще двух, и он автоматически обслуживает гонку переиспользования имени секции
(§6.3). `MaxObjects` в `PcAddAdapterDevice` поднимается 4 → **20**
(10 endpoint-слотов × 2 субдевайса).

### 6.2 FriendlyName — пользовательское имя endpoint'а

Проблема: INF AddInterface статичен, а endpoint должен называться «Nodus Game Audio».

**Основной путь — запись из kernel.** После `PcRegisterSubdevice` интерфейсы субдевайса
уже зарегистрированы PortCls. Драйвер: `IoGetDeviceInterfaces(Pdo, &KSCATEGORY_AUDIO,
DEVICE_INTERFACE_INCLUDE_NONACTIVE, &list)` → найти symlink, чей reference string
совпадает с именем субдевайса («…\Wave-3») → `IoOpenDeviceInterfaceRegistryKey(…,
KEY_SET_VALUE, …)` → `ZwSetValueKey("FriendlyName", REG_SZ, имя из CREATE)`. Пишем для
обоих интерфейсов пары (имя endpoint'а наследуется от topology-фильтра, но симметрия
дешёвая). Kernel-mode запись идёт от SYSTEM — ACL ключей DeviceClasses не препятствие,
elevation userspace не нужен. Это ровно то же значение, которое сейчас пишет INF
(`HKR,,FriendlyName`), т.е. потребитель (AudioEndpointBuilder) уже доказанно его читает.

**Fallback — имена-слоты в INF.** В INF добавляются AddInterface/AddReg-секции для всех
32 динамических имён (`Wave-1..8`, `Topology-1..8`, `WaveCap-1..8`, `TopologyCap-1..8`)
с категориями как у статики и дефолтным `FriendlyName = "Nodus Virtual Device N"` +
`CLSID` proxy. SetupAPI создаёт interface-ключи при установке, регистрация в рантайме их
активирует (прецедент — SYSVAD INF с sideband-именами). Если запись из kernel не удалась
(логируем, не фатально) — endpoint появляется с внятным дефолтным именем, а не безымянным.

**Отвергнуто — PKEY_Device_FriendlyName через MMDevice из userspace**: запись в
`HKLM\...\MMDevices` требует elevation — ломает основное требование.

Переименование живого устройства в MVP = DESTROY + CREATE с тем же id (UI делает это
прозрачно); отдельный SET_NAME — кандидат в Phase 2, протокол расширяем (Reserved/новый
IOCTL-код). Полевой пункт проверки: кэш свойств endpoint'а в MMDevices — убедиться, что
при пересоздании с тем же id новое имя подхватывается (AudioEndpointBuilder перечитывает
FriendlyName при появлении интерфейса; если на каком-то билде Windows нет — fallback:
смена id при rename).

### 6.3 DESTROY — порядок teardown

Под тем же мутексом:

1. Найти запись; статика (id 0) и отсутствующие id отбиваются.
2. QI у портов `IUnregisterPhysicalConnection` → снять физсвязь;
   `IUnregisterSubdevice::UnregisterSubdevice` для wave и topo (паттерн SYSVAD
   `UnregisterAllSubdevices`). Endpoint исчезает из MMDevice, новые открытия пинов
   невозможны.
3. `Release()` наших ссылок на порты, слот освобождается, выход из мутекса, успех.

Чего DESTROY **не делает** — и почему это безопасно:

- **Не убивает стримы и копировальные потоки.** audiodg может держать пин. Цепочка
  владения уже выстроена в t2/t3: стрим держит ссылку на миниport; копировальный поток
  join'ится в `FreeAudioBuffer` ДО освобождения буфера; кольцо принадлежит миниporty и
  уничтожается в его деструкторе. После unregister PortCls инвалидирует фабрику, audiodg
  закрывает пины, последний Release стрима → деструктор миниporta (PASSIVE) →
  `NodusRingDestroy`. Ни одна сторона не пишет в кольцо после его смерти по построению.
- **Не гарантирует мгновенную смерть секции.** Если CREATE с тем же id придёт раньше,
  чем audiodg отпустил старый стрим, `ZwCreateSection` нового кольца получит
  `STATUS_OBJECT_NAME_COLLISION` — EnsureRing и так ленивый и повторяется на каждом
  NewStream, коллизия самоустраняется. Документируем как штатное поведение.

---

## 7. Лимиты, id, гонки

| Параметр | Значение | Обоснование |
|---|---|---|
| Максимум динамических | 8 | Покрывает MVP «3+» с запасом; 8 колец = ~3 МБ секций; UI-список остаётся обозримым |
| Пространство id | 1..8, единое для render и capture | id 0 — статика (оба кольца NodusRing-0 и NodusRing-mic-0 заняты ею); уникальный id независимо от kind делает DESTROY(id) однозначным; коллизий имён секций нет, т.к. шаблон выбирается по kind |
| Переиспользование id | разрешено сразу после DESTROY | гонка имени секции самоустраняется (§6.3) |
| Сериализация | один KMUTEX в контексте адаптера на CREATE/DESTROY/LIST-снимок и PnP-remove | операции редкие, контеншена нет; всё PASSIVE |
| Назначение id | RequestedId либо минимальный свободный | явный id обязателен для персистентности (§8) |

Стабильность endpoint-идентичности: MMDevice выводит endpoint ID из device interface +
pin, поэтому пересоздание устройства с тем же reference-именем (`WaveCap-3`) после ребута
даёт **тот же endpoint ID** — Discord/OBS, запомнившие устройство, привязываются обратно.
Именно поэтому CREATE принимает RequestedId.

---

## 8. Персистентность

**Решение (MVP): драйвер не хранит ничего. Восстанавливает Nodus.**

- Конфиг приложения (рядом с существующим сохранением сцен) получает секцию
  `virtual_devices: [{ id, kind, name }]`.
- При старте Nodus: найти control-интерфейс (с ретраем — devnode может ещё стартовать) →
  `QUERY_VERSION` → `LIST_DEVICES` → diff с конфигом → `CREATE_DEVICE(RequestedId=id)`
  для отсутствующих → событие `virtual-devices-changed`.
- Тот же путь чинит ситуацию «обновили драйвер → devnode перезапустился → динамика
  пропала» (известные грабли t2: рестарт устройства при апдейте).

Почему не реестр + восстановление в StartDevice (вариант из заметок t5):

1. StartDevice выполняется на ранней загрузке — ровно там, где уже ловили
   PATH_NOT_FOUND для `\BaseNamedObjects`; восстановление субдевайсов там работает, но
   запись/чтение пользовательских имён и порядок с INF-fallback'ом усложняются.
2. Двойное владение состоянием (конфиг Nodus + реестр драйвера) гарантирует
   рассинхронизацию; источник истины должен быть один, и им всё равно остаётся Nodus
   (имена, привязка к нодам канваса).
3. Деинсталляция проще: убрали devnode — динамики не существует, хвостов в реестре нет.

Цена решения: после ребута до запуска Nodus видна только статическая пара; приложение,
стартовавшее раньше Nodus (Discord в автозапуске), увидит своё устройство с задержкой
в секунды. MMDevice обрабатывает горячее появление, привязка восстанавливается благодаря
стабильному endpoint ID (§7). Без работающего Nodus динамическое устройство всё равно
бесполезно (кольцо некому обслуживать), так что окно невидимости ничего не отнимает.
Если в поле выяснится, что какое-то приложение не переподхватывает устройство —
план Б: автозапуск лёгкого Nodus-агента, НЕ перенос состояния в драйвер.

---

## 9. Жизненный цикл, сценарии BSOD-риска и их исключение

| # | Сценарий | Чем исключается |
|---|---|---|
| 1 | Копировальный поток пишет в кольцо после его уничтожения | Владение: кольцо умирает только в деструкторе миниporta; стрим join'ит поток до освобождения буфера и держит миниport ссылкой (существующий инвариант t2) |
| 2 | DESTROY при активном пине audiodg → use-after-free портов | Наши Release — только ПОСЛЕ IUnregisterSubdevice; дальше временем жизни управляет refcount PortCls/KS |
| 3 | Гонка CREATE/DESTROY на одном id | Глобальный KMUTEX на все мутации таблицы |
| 4 | CREATE/DESTROY во время IRP_MN_REMOVE_DEVICE | PNP-хук: под тем же мутексом выставить флаг `Removing`, снять все динамические пары (тот же teardown, что DESTROY), погасить control-интерфейс, затем PcDispatchIrp; IOCTL после флага → STATUS_DEVICE_NOT_READY |
| 5 | Кривой ввод из userspace (короткий буфер, имя без NUL, Kind=42) | METHOD_BUFFERED + полная валидация до каких-либо аллокаций (§5); фиксированные размеры, нет указателей в структурах |
| 6 | Частично выполненный CREATE (второй InstallSubdevice упал) | Атомарный откат под мутексом: unregister успевшего, Release, слот свободен |
| 7 | Коллизия имени секции при быстром пересоздании id | Ленивый EnsureRing с ретраем на каждом NewStream — самоизлечивается без участия протокола |
| 8 | IRQL-нарушения | Весь control-plane PASSIVE_LEVEL (DEVICE_CONTROL dispatch, PcRegisterSubdevice, реестр, секции); PAGED_CODE-ассерты как в ring.cpp |
| 9 | Исчерпание ресурсов злоупотреблением IOCTL | Жёсткий лимит 8, фиксированные структуры, нет аллокаций, растущих от пользовательского ввода |

Бюджет производительности: протокол — чистый control-plane. В горячем пути аудио
(CopyLoop, 10 мс тики) изменений нет; мутекс протокола в аудиопути не берётся. Влияние
на задержку — ноль.

---

## 10. Миграция со статики

**Выбран вариант (а): статическая пара остаётся «устройством 0», динамика — сверху.**

- Boot-путь t1–t4, проверенный в поле тремя итерациями, не трогаем вообще.
- Установка одного драйвера без Nodus по-прежнему даёт видимые Speaker+Mic — это рабочий
  smoke-тест на тестовом ноуте и гарантия, что endpoint-механика жива до любых IOCTL.
- ring-check / ring-tone / ring-play и `virtual_capture.rs` / `virtual_render.rs`
  (id = 0) работают без изменений.
- LIST показывает статику двумя записями `{Id=0, Kind=Render|Capture,
  NODUS_DEVFLAG_STATIC}`; DESTROY(0) запрещён. UI рисует их как встроенные.
- Деинсталляция: динамика умирает вместе с devnode (субдевайсы и секции — это рантайм),
  остаётся обычный `pnputil /delete-driver`; конфиг Nodus чистится приложением.

Вариант (б) — пустой StartDevice, дефолтную пару создаёт Nodus через IOCTL — отвергнут:
обнуляет полевую валидацию t1–t4, делает install-скрипты непроверяемыми без приложения,
первый запуск зависит от гонки «devnode стартовал vs приложение успело». Возможная
унификация «всё динамическое» имеет смысл только после того, как динамика проживёт
в поле несколько релизов.

---

## 11. Rust-сторона (эскиз)

Новый модуль `src-tauri/src/audio/device_control.rs`:

```rust
// Зеркало nodus_ioctl.h: repr(C)-структуры + offset-тесты (паттерн ring_layout.rs).
pub const CTL_VERSION: u32 = 1;
pub const GUID_DEVINTERFACE_NODUS_CONTROL: GUID = ...; // тот же GUID, что в nodus_ioctl.h

pub enum DeviceKind { Render, Capture }            // <-> NODUS_KIND_*

pub struct VirtualDeviceInfo {                     // typed-ответ для UI
    pub id: u32,
    pub kind: DeviceKind,
    pub name: String,
    pub is_static: bool,
    pub ring_active: bool,
}

pub struct DeviceControl { handle: OwnedHandle }   // CreateFileW по interface path

impl DeviceControl {
    pub fn open() -> Result<Self, ControlError>;   // CM_Get_Device_Interface_ListW + CreateFileW
    pub fn query_version(&self) -> Result<u32, ControlError>;   // отказ при mismatch
    pub fn create(&self, kind: DeviceKind, requested_id: Option<u32>, name: &str)
        -> Result<u32, ControlError>;              // -> id
    pub fn destroy(&self, id: u32) -> Result<(), ControlError>;
    pub fn list(&self) -> Result<Vec<VirtualDeviceInfo>, ControlError>;
}
// внутри: единый fn ioctl<I: Pod, O: Pod>(&self, code: u32, input: &I) -> Result<O>
// через DeviceIoControl (windows crate); NTSTATUS-ошибки -> ControlError с текстом.
```

Команды в `src-tauri/src/commands/bridge.rs` (стиль существующих):

```rust
#[tauri::command]
pub async fn create_virtual_device(name: String, kind: String /* "render"|"capture" */,
                                   app: AppHandle) -> Result<u32, String>;
#[tauri::command]
pub async fn remove_virtual_device(id: u32, app: AppHandle) -> Result<(), String>;
#[tauri::command]
pub async fn list_virtual_devices() -> Result<Vec<VirtualDeviceInfo>, String>;
```

Обе мутирующие команды: после успеха — обновить секцию `virtual_devices` конфига и
`app.emit_all("virtual-devices-changed", list)`. Восстановление при старте — в setup-пути
приложения (рядом с инициализацией движка): open → query_version → list → создать
недостающие из конфига → событие. DeviceIoControl — блокирующий вызов, оборачивать в
`tokio::task::spawn_blocking` (как start_engine/stop_engine).

Совместимость: `virtual_capture.rs` / `virtual_render.rs` сейчас жёстко на id 0 — в t5
не трогаются (статика работает как раньше). Обобщение «движок открывает кольцо по id
устройства из графа» — следующий шаг t5 (см. план, шаг 7), имена секций уже
параметрические с обеих сторон.

---

## 12. Отвергнутые альтернативы (сводно)

| Альтернатива | Причина отказа |
|---|---|
| Devnode на устройство | Требует elevation на каждое создание (решено ранее, вне обсуждения) |
| Отдельный control-device (IoCreateDeviceSecure) | Второй жизненный цикл и symlink ради SDDL, который нам не нужен; хук — прецедент SYSVAD |
| Custom KS property set на topology-фильтре | Та же доступность, но больше plumbing'а (KS property handler, file object семантика); IOCTL по своему GUID проще и явнее |
| FriendlyName через MMDevice/SetupDi из userspace | Запись требует elevation — ломает ключевое требование |
| Персистентность в реестре драйвера + восстановление в StartDevice | Ранняя загрузка, двойное владение состоянием, хвосты при деинсталляции (§8) |
| CREATE создаёт пару render+capture за раз | Доменная модель Nodus различает Virtual Output и Virtual Mic как отдельные сущности; пара навязывала бы лишнее |
| LIST с variable-size ответом | 10 слотов × 144 байта — копеечный фиксированный буфер, нулевая логика продолжений |
| RENAME ioctl в MVP | DESTROY+CREATE с тем же id покрывает; код в драйвере минимальнее |

---

## 13. План реализации

Каждый шаг компилируется в CI (драйвер собирается и подписывается тест-сертификатом,
`cargo test` зелёный); «поле» = тестовый ноут.

| Шаг | Содержание | Кто | Готово, когда | CI / поле |
|---|---|---|---|---|
| 1 | `nodus_ioctl.h` (коды, GUID, структуры, C_ASSERT оффсетов) + зеркало в `device_control.rs` с offset-тестами | driver + backend | C_ASSERT и cargo-тесты фиксируют одинаковые оффсеты с двух сторон | CI полностью |
| 2 | Хуки DEVICE_CONTROL/PNP, регистрация control-интерфейса, QUERY_VERSION + LIST (отдаёт только статику) | driver | CLI-утилита `device-ctl.exe` (в ряд к ring-check) открывает интерфейс, печатает версию и 2 статических устройства; звук статики не деградировал | CI: сборка; поле: smoke |
| 3 | Таблица адаптера, CREATE/DESTROY для render: параметризация фабрик по RingId, InstallSubdevice с Irp=NULL, откат, MaxObjects=20 | driver | device-ctl create render → endpoint виден в Windows, ring-tone/ring-play работают с новым id; destroy убирает endpoint; повторить ×8, лимит отбивается | поле |
| 4 | FriendlyName из kernel + INF-слоты (32 AddInterface) | driver | Созданное устройство называется заданным именем; при намеренно сломанной записи — именем слота; rename через destroy+create(same id) обновляет имя | поле (кэш MMDevices!) |
| 5 | CREATE/DESTROY для capture + DESTROY под активным стримом (audiodg держит пин, Discord пишет с мика) | driver | Нет BSOD при destroy во время записи/воспроизведения; пересоздание того же id при живом audiodg самовосстанавливается; PNP-remove (отключение в диспетчере) с живой динамикой чисто | поле, стресс |
| 6 | `device_control.rs` + 3 команды в bridge.rs + событие + персистентность в конфиге + восстановление при старте | backend | Из UI создаются 3+ устройства без UAC; ребут → Nodus стартует → устройства вернулись с теми же id/именами; Discord переподхватывает мик | CI: unit; поле: e2e |
| 7 | Движок: открытие колец по id из графа (обобщение virtual_capture/virtual_render), маппинг endpoint↔id в virtual_device.rs | backend | Звук двух разных динамических устройств не смешивается (критерий t5 «у каждого своё кольцо») | поле |

Критерии готовности t5 (из задачи) закрываются: шаг 6 — «3+ устройств без UAC» и
«переживают перезагрузку», шаги 3+5 — «удаление освобождает ресурсы», шаг 7 — «звук не
перемешивается».

Риски, требующие полевой проверки (не решаются за столом): поведение кэша
AudioEndpointBuilder при пересоздании id с новым именем (шаг 4); реакция конкретных
приложений (Discord/OBS) на горячее появление endpoint'а после ребута (шаг 6);
стабильность `PcRegisterSubdevice` после StartDevice на минимально поддерживаемой
Win10 1709 (шаг 3 — прогнать на старейшей доступной системе).
