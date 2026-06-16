# Nodus agent team — how this is wired

Adapted from the `claude-agent` multi-agent framework (D:\claude\claude-agent) to fit
how Nodus actually works. Key local differences from the original framework:

- **The main Claude Code session is the Team Lead.** There is no separate teamlead
  agent; the session orchestrates, talks to the human (in Russian), authors tasks,
  and reviews reports.
- **Tasks live in `.nodus/task/`** in the human-readable Russian format already in
  use (`backlog.md` + `t{N}-<slug>.md`). Do not rename to the framework's
  `t{N}-{project}-{date}` scheme — the human reads this board.
- **Reports go to `.nodus/report/r{N}-<slug>-<DDMMYYYY>.md`** (English or Russian,
  always with a short Russian summary at the top). Chat return = status + summary +
  report path + changed files.
- **`.nodus/` is committed** in this repo (the framework gitignores it; we don't).
- **No `concept` agent** — the product idea, scope, and MVP are settled in CLAUDE.md.
- Project context: agents read the repo `CLAUDE.md` (auto-loaded), the task file,
  and `.nodus/` docs relevant to the task. There is no separate PROJECT.md.

## Roles

| Need | Agent |
|------|-------|
| Kernel audio driver: C++/PortCls/WaveRT, INF, driver CI | `driver` |
| Rust backend: WASAPI, routing engine, Tauri commands | `backend` |
| Wiring logic into existing React UI (no visual changes) | `ui-bridge` |
| Verification, bug reproduction, review verdicts | `qa` |
| Design decisions, ADRs, contracts before code | `architect` |
| Internal docs, keeping `.nodus/` docs and statuses current | `docs` |

## Orchestration rules (Team Lead = main session)

- Max **3 agents in parallel** (CLAUDE.md rule #7), and only when their change zones
  don't overlap; never let two agents write the same file.
- One task = one owner role = one coherent change. Vague task → vague result.
- A task file an agent works from must state: goal, allowed/forbidden paths,
  acceptance criteria. Verify reports against the criteria, not the narrative.
- Non-trivial changes go through `qa` review mode before being called done.
- Specialists never edit `backlog.md` — they raise follow-ups in their report's
  Handoff section; the Team Lead triages into the backlog.
