# r1 — t3: виртуальный микрофон (capture-путь в kernel-драйвере)

**Статус:** код написан и перепроверен; сборка НЕ запускалась (EWDK только в CI — пуш делает Team Lead).

Добавлен capture-путь «Nodus Virtual Mic»: вторая пара субдевайсов WaveCap+TopologyCap (зеркало render), отдельное мир-записываемое кольцо `Global\NodusRing-mic-0`, PASSIVE-поток заполнения capture-буфера (звук из кольца от Nodus, остаток — тишина), graceful degradation в StartDevice (если capture не встал — динамик поднимается всё равно); render-путь не тронут нигде, кроме сигнатуры NodusRingCreate.

## Файлы

Новые:
- `src-tauri/driver/nodus_audio/minwavecap.h/.cpp` — WaveRT capture миниnopт: фильтр host(OUT,SINK)+bridge(IN), категории {AUDIO, CAPTURE, REALTIME}; владеет mic-кольцом (EnsureRing лениво под KMUTEX, WorldWritable=TRUE), арбитраж single-consumer ClaimReader/ReleaseReader.
- `src-tauri/driver/nodus_audio/minwavecapstream.h/.cpp` — capture-стрим: NonPaged буфер+MDL, GetPosition по времени, системный поток FillLoop каждые 10 мс (кольцо → циклический буфер, остаток — RtlZeroMemory), teardown сигнал→join→деref→ZwClose→буфер.
- `src-tauri/driver/nodus_audio/mintopocap.h/.cpp` — топология микрофона: mic-connector (KSNODETYPE_MICROPHONE, IN) → bridge (OUT), категории {AUDIO, TOPOLOGY}.

Изменённые:
- `nodus.h` — имена WaveCap/TopologyCap, pin-enum'ы WAVECAP_*/TOPOCAP_*, две новые фабрики.
- `common.h` — NODUS_RING_MIC_NAME_KERNEL + комментарий о реверсе ролей счётчиков (WriteBytes пишет userspace, ReadBytes — драйвер).
- `ring.h/ring.cpp` — NodusRingCreate(NameTemplate, Id, WorldWritable, Ring); при WorldWritable Everyone-ACE получает SECTION_MAP_WRITE.
- `minwavert.cpp` — только обновлён вызов: NodusRingCreate(NODUS_RING_NAME_KERNEL, 0, FALSE, ...).
- `adapter.cpp` — MaxObjects 2→4; StartDevice ставит WaveCap+TopologyCap и PcRegisterPhysicalConnection(topoCap TOPOCAP_PIN_BRIDGE → waveCap WAVECAP_PIN_BRIDGE); ошибка capture-половины логируется, но не валит StartDevice.
- `nodus_audio.inf` — AddInterface для WaveCap (AUDIO/CAPTURE/REALTIME → "Nodus Virtual Mic") и TopologyCap (AUDIO/TOPOLOGY → "Nodus Virtual Mic Topology"), строки KSCATEGORY_CAPTURE/KSNAME_WaveCap/KSNAME_TopologyCap/WaveCap.Desc/TopoCap.Desc. DriverVer не трогал.
- `nodus_audio.vcxproj` — три новых .cpp в ClCompile, три .h в ClInclude.

## Решения и риски

1. **Поток заполнения работает и без кольца.** В отличие от render (там без кольца поток не нужен — звук просто дропается), микрофон обязан отдавать валидные сэмплы. Буфер из ExAllocatePool2 нулевой с рождения, и только наш поток в него пишет, так что тишина гарантирована даже без потока — но поток оставлен всегда-живым для единообразия и на случай будущего «кольцо появилось позже».
2. **Ресинк кольца.** При avail > 3/4 кольца ReadBytes перепрыгивает на WriteBytes − 9600 (~50 мс от свежего края, выровнено по блоку). Это же автоматически отбрасывает накопленный «застойный» звук при старте стрима (Nodus писал часами, стрим только открылся → avail огромный → ресинк на первом тике). Дополнительно: если WriteBytes < ReadBytes (userspace сбросил счётчик вопреки контракту) — снап ReadBytes = WriteBytes, тишина вместо мусора.
3. **GetPosition строго по часам, заполнение — по тем же часам с лагом до 10 мс.** Клиент читает позади позиции; данные у самой кромки позиции могут быть дописаны на ~10 мс позже. Стандартные WASAPI-клиенты читают пакетами с задержкой ≥ периода — риск считаю низким, но если в поле будет слышен «рваный» край, лечится упреждением (target += 10 мс байт) — однострочная правка.
4. **Direction физсвязи** для capture — из topology в wave (bridge topo OUT → bridge wave IN), зеркально render; пин-направления в дескрипторах согласованы с этим.
5. **Whole frames only:** avail и take выравниваются по NODUS_BLOCK_ALIGN — приложение никогда не получит полкадра, даже если userspace опубликовал WriteBytes посреди кадра.

## IRQL / lifetime walkthrough

- **IRQL:** NodusRingCreate/Destroy — PASSIVE (PAGED_CODE, вызовы из Init/NewStream/деструктора миниnopта). FillLoop — выделенный системный поток на PASSIVE_LEVEL, единственное место, где трогаются ring-view и буфер; никаких DPC/таймеров в драйвере по-прежнему нет. SetState/GetPosition не трогают ring-view вообще (как в render).
- **Lifetime кольца:** стрим держит AddRef на миниnopт с Init до деструктора; миниnopт рушит кольцо только в своём деструкторе, когда все стримы (и их потоки) уже ушли. Поток join'ится в FreeAudioBuffer/деструкторе стрима ДО освобождения буфера и ДО Release миниnopта — поток не может пережить ни буфер, ни ring-view.
- **Teardown потока:** KeSetEvent → KeWaitForSingleObject(threadObj) → ObDereferenceObject → ZwClose; если ObReferenceObjectByHandle не удался — потоку сразу сигналится стоп (join невозможен, но поток гарантированно выходит сам, а буфер в этом ветвлении ещё жив до FreeAudioBuffer, который без m_ThreadObject просто закроет handle — здесь паттерн идентичен полевому render-коду).
- **Refcount:** фабрики AddRef'ают (stdunk стартует с 0), InstallSubdevice не менялся; NewStream отдаёт референс caller'у, port->Init держит свой реф на миниnopт.
- **Арбитраж:** single-consumer через InterlockedCompareExchangePointer — два транзиентных стрима не дерутся за ReadBytes: не-claimant отдаёт чистую тишину.

## Верификация

- CI-сборка НЕ запускалась: пуш вне моей зоны (Team Lead коммитит после ревью). Перед мержем обязательны: компиляция EWDK, InfVerif (новые секции INF — основной риск опечатки), подпись.
- Выполнена ручная сверка: сигнатуры IMP_IMiniportWaveRT/IMP_IMiniportWaveRTStream/IMP_IMiniportTopology и NonDelegatingQueryInterface — копия проверенных render-файлов; имена nodus.h ↔ adapter.cpp ↔ INF сходятся (grep-проверка); пин-id ↔ PcRegisterPhysicalConnection согласованы; C_ASSERT'ы layout не тронуты; render-файлы кроме одного вызова не изменены.

## План ручного теста (тестовый ноут, Test Mode)

1. Поставить свежий CI-артефакт: `install.ps1` (версия поднимется — должен пойти update-путь). Перезагрузка не обязательна, но при странностях — перезагрузить.
2. **Параметры → Звук:** во вкладке «Вывод» по-прежнему «Nodus Virtual Speaker» (регрессия render — первое, что проверяем); во вкладке «Ввод»/«Запись» должен появиться **«Nodus Virtual Mic»**.
3. DebugView (kernel capture): искать строки `Nodus: PcNewPort(WaveCap)`, `PcRegisterSubdevice(WaveCap/TopologyCap)`, `PcRegisterPhysicalConnection(capture) status=0x00000000`, `StartDevice end ... (capture=0x00000000)`.
4. Открыть «Запись голоса» (Voice Recorder), выбрать Nodus Virtual Mic устройством по умолчанию, записать 10 секунд → дорожка должна быть **ровной тишиной** (не шум, не мусор, запись не виснет). В DebugView при старте записи: `Nodus: NodusRingCreate(mic, 0) status=0x00000000` (или уже создано), `capture NewStream`, `capture PsCreateSystemThread status=0x00000000`.
5. Стресс подключений: 5–10 раз подряд старт/стоп записи; параллельно открыть Discord и выбрать Nodus Virtual Mic; закрыть Discord во время «передачи». Ничего не должно падать; в Sound Settings устройство остаётся.
6. Проверка кольца: `ring-check` CLI (backend, t2) пока знает только render-кольцо; запись звука В микрофон появится после backend-задачи (зеркало virtual_capture.rs — пишущая сторона). Для t3 достаточно тишины и стабильности.
7. При BSOD: зафиксировать bugcheck-код и смещение `nodus_audio.sys+offset` (PDB в CI-артефакте), минидамп забрать.

## Handoff

- **Team Lead:** ревью → коммит → пуш → CI (компиляция + InfVerif) → полевой тест по плану выше.
- **backend:** нужна пишущая сторона mic-кольца в Rust: открыть `Global\NodusRing-mic-0` с FILE_MAP_WRITE, валидировать Magic/Version=2 (layout идентичен render-кольцу), писать PCM 48/2/16 в Data[WriteBytes % RingBytes] и публиковать WriteBytes ПОСЛЕ данных (release-барьер); ReadBytes не трогать — он драйверский. Держать опережение записи небольшим (~50–100 мс), иначе ресинк драйвера будет резать хвост.
