# HANDOFF — продолжить здесь (на 14.06.2026)

Короткая записка для следующей сессии (и для меня-человека). Полный контекст
проекта — в `START_PROMPT.md` и `CLAUDE.md`; доска задач — `.nodus/task/backlog.md`.

## Где мы сейчас

Идёт **редизайн UI (задача t11)** — новый стек **TypeScript/TSX** в `src/`.
Вход: `index.html → src/main.tsx → src/NodusApp.tsx`. Старый `src/*.jsx` (гиперскрипт)
больше НЕ грузится, оставлен для справки при порте логики.

Драйвер/движок (t1–t5) — отдельный фронт, на паузе ради редизайна. Их статус — в backlog.

## Что сделано по редизайну (детали — `.nodus/task/t11-ui-redesign.md`)

- **R1–R2** тулчейн TS + токены/палитра + фон холста.
- **R4–R6** нода (NodeCard), типы + hub «Stream Mix» (HubNode), состояния, свечение по курсору.
- **R8** рёбра (Graph; порты меряются из DOM).
- **R10–R15** топбар, кнопка движка, bottom bar (список в середине + поиск подсвечивает холст),
  «+ add» панель, зум, статус.
- **R16–R17** пустой холст (EmptyCanvas + scenes.ts) + онбординг-модалка.
- **R3** мост: `src/bridge.ts` (типы+invoke+события по Rust-контракту), `src/useBackend.ts`
  (реальные устройства/процессы/levels + start/stop, browser-fallback). AddPanel и кнопка
  движка уже на реальных данных. Метры нод подключены к live-levels (R18-начало).

## Что делать дальше — R18 (довести макет до рабочего приложения)

Контракт к Rust — в `src/bridge.ts`. По порядку:
1. **Создание ноды** из «+ add» и пресетов с реальными `device_id`/`exe_name`
   (сейчас сцены образцовые без id). AudioDevice/AudioProcess → NodeModel.
2. **Сборка `RoutingGraph`** (BackendNode/BackendRoute) из нод+рёбер холста →
   `applyRoutingGraph(graph)` при включении движка.
3. **Слайдеры** громкости/mute/pan на ноде и поповере ребра → `setRouteVolume/Mute/Pan`.
4. **Save/load** сцен (export/import JSON).
5. Реальная установка драйвера из модалки R17; уведомление **t7** («Nodus не слышит систему»).
- Параллельно мелочь: R5-хвост (fx/logic-ноды), R7 (реальные иконки — backend HICON), R9
  (поповер ребра), R19 (перф-проход).

## Как запускать / проверять
- UI в браузере: `npm run dev` (Vite :1420) — без Tauri, данные из browser-fallback.
- Типы: `npx tsc --noEmit`. Полное приложение: `npm run tauri dev`.
- Грабля: Vite HMR в preview часто отдаёт стейл-версию после ошибки → перезапуск +
  `rm -rf node_modules/.vite` + reload. `backdrop-filter:blur` не используем (перф + скриншоты).

## Источники истины по дизайну
`.nodus/docs/ReDesign/redesign-instructions.md` (главная) + код-прототип в
`.nodus/docs/ReDesign/Claude-design/design-prototype/src/` (цвета/поведение). PNG-картинки —
концепты, не всегда совпадают с кодом. Палитра уже в `src/styles/tokens.css`.
