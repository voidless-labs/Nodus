# r2 — t4: движок пишет в виртуальный микрофон (VirtualRender)

Выжимка: Rust-движок научился наполнять виртуальный микрофон — маршруты с
назначением «Nodus Virtual Mic» пишут звук в кольцо драйвера
(`Global\NodusRing-mic-0`) вместо WASAPI; громкость/mute/pan маршрута работают
как везде; без установленного драйвера маршрут жив, просто молчит. Код — агент
backend; верификация — Team Lead (агенту не дали Bash): `cargo build` чисто,
`cargo test` **51 passed / 0 failed** (+14 новых).

## Файлы

Новые:
- `src-tauri/src/audio/ring_layout.rs` — единое Rust-зеркало контракта
  NODUS_RING_BUFFER v2 (RingHeader, константы, имена секций, layout-тесты).
  Раньше зеркало жило в virtual_capture.rs; теперь одна копия на оба направления.
- `src-tauri/src/audio/virtual_render.rs` — VirtualRender (API зеркален
  AudioRenderer): поток-писатель с TimerResolutionGuard; открытие секции внутри
  потока (нет драйвера → один warn, поток выходит, маршрут жив); DSP
  mute/vol/pan в семантике run_render; mono/multi → stereo; MVP-линейный
  ресемплер → 48 кГц; f32 → i16 с клиппингом; запись в кольцо двумя сегментами
  через wrap + Release-fence + volatile write_bytes (продолжение от текущего
  счётчика секции). Опережение читателя на полное кольцо — штатно (никто не
  записывает с микрофона; драйвер сам ресинкнется).

Изменённые:
- `virtual_capture.rs` — переведён на общий ring_layout (поведение не менялось).
- `virtual_device.rs` — `is_nodus_virtual_mic_name()` («nodus» И
  («mic»|«микрофон»), case-insensitive) + тесты позитив/негатив.
- `routing/graph.rs` — `ActiveRoute.to_is_virtual_mic` по label узла-назначения;
  тест: Mic → true, «Наушники»/«Nodus Virtual Speaker» → false.
- `routing/engine.rs` — `enum RouteSink { Wasapi(AudioRenderer),
  VirtualMic(VirtualRender) }`; `RouteHandles.renderer` → `sink`; выбор по
  `to_is_virtual_mic`; stop_internal гасит оба вида; set_route_* работают через
  общие атомики без изменений.
- `audio/mod.rs` — регистрация модулей.

## Решения
- Общее зеркало контракта вынесено в ring_layout.rs — двух копий больше нет,
  layout-тест один.
- Открытие секции в потоке, а не в start(): сохраняет сигнатуру AudioRenderer
  и даёт ленивую устойчивость к отсутствию драйвера.
- Ресемплер — линейная интерполяция, без переноса фазы между кадрами; помечен
  как MVP (источники движка обычно и так 48 кГц).

## Верификация
- `cargo build` — без ошибок и новых warnings.
- `cargo test` — 51 passed, 0 failed (layout, конверсии, DSP, ресемплер,
  graph-маркировка, все старые).
- Рантайм с реальным кольцом НЕ проверялся (тестовый ноут недоступен) —
  честно: не проверено.

## Полевой тест (когда вернётся ноут; вместе с t3)
1. Артефакт CI №20+ → install.ps1 → «Nodus Virtual Mic» во вкладке Запись (t3).
2. ring-tone.exe → тон в «Записи голоса»/Discord (t3, кольцо живо).
3. Nodus: маршрут «Spotify → Nodus Virtual Mic» (нода-назначение с этим именем),
   ENGAGE → Discord с выбранным Nodus Virtual Mic слышит музыку; громкость/mute
   ребра действуют (t4). Это вторая половина MVP-сценария.
