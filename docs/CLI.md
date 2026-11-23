# neati command line and agent skill (#498)

`neati` runs from a terminal, and coding agents such as Claude Code and Codex
can drive it through the bundled skill.

## Ports and adapters

The CLI is a driving adapter. It calls interface-agnostic use cases through
ports and never calls a service directly.

```text
 driving adapters          driving ports              application core                 driven ports              driven adapters
 ────────────────          ─────────────              ────────────────                 ────────────              ───────────────
 cli/ (terminal,     ──▶   CleanUseCase       ──▶   OneButtonClean                ──▶  CacheCleanup     ──▶  ServiceCacheCleanup → CleanupService
 agents via skill)         InsightQueries            (scan both, choose, narrow,       ProjectCleanup   ──▶  ServiceProjectCleanup → StorageService
 commands/ (window,        ActivityPort ◀────────    execute, merge)
 future)                                             selection.rs (pure)
                                                     MachineInsights ─────────────────────────────────▶  System/Cleanup/AI services
```

The pieces live in these places:

- `services/ports.rs` defines `PortFuture`, `Activity` and `ActivityPort`.
  These are shared by every use case. No port names Tauri, a terminal or stdout.
- `services/one_button_clean/` holds the use case.
  - `ports.rs` has the driving port (`CleanUseCase`) and the driven ports
    (`CacheCleanup`, `ProjectCleanup`), with plain request and outcome types.
  - `selection.rs` names and narrows the set. It is pure.
  - `mod.rs` orchestrates the run.
  - `adapters.rs` is the only file that names a concrete service.
- `services/insights.rs` holds the read-only driving port (`InsightQueries`)
  and `MachineInsights`.
- `composition/interfaces.rs` is the only place a driven port is bound to an
  adapter. `application_ports(&DesktopState)` returns `ApplicationPorts`.
- `cli/context.rs` is the CLI's composition root. It reads the stored
  settings, builds `DesktopState` and keeps only `ApplicationPorts`.
- `cli/*` holds the handlers. They own the grammar, approval (`--yes` or a
  person answering the prompt), terminal progress (an `ActivityPort`) and the
  `neati.cli/1` JSON shapes. The JSON types live here, not in the use case, so
  the wire format cannot change by accident.

`src-tauri/tests/port_boundary_tests.rs` turns this into a build invariant.
It fails if:

- a CLI handler names `DesktopState` or a service,
- the use case core (`mod.rs`, `ports.rs`, `selection.rs`) names a service,
  Tauri or stdout, or
- a driven port is bound outside `composition/`.

The Quick Panel already uses this use case (#500). Two narrow commands serve
it:

- `preview_one_button_clean` holds a `CleanPreview` in `OneButtonSessions`
  behind an opaque one-shot id, with a 5-minute TTL and a cap of 4. It
  reuses the scan the panel just published while that scan is valid for
  cleanup.
- `execute_one_button_clean` runs exactly that preview, once.

The panel no longer re-derives which caches are cleanable. The dashboard's
Clean keeps its explicit per-item selection, because reviewable items need a
person's choice.

## Where the command comes from

The CLI is the app's own `Neati` executable. `main.rs` hands the arguments to
`cli::run` before Tauri starts. Only a recognised command word (`status`,
`scan`, `projects`, `clean`, `ports`, `ai`, `history`, `doctor`, `skill`,
`sessions`, `help`, `version`) is the CLI's. Any other launch is the app's, including no
arguments, the login item's `--background` and `--doctor`.

- With Homebrew, the cask links the executable onto `PATH` as `neati`.
- Without Homebrew, call `/Applications/neati.app/Contents/MacOS/Neati <command>`,
  or link it yourself.

The CLI is macOS-first. Windows release builds use the GUI subsystem and have no
console, and Windows support is paused (#445).

## Commands

| Command | Effect |
| --- | --- |
| `status` | Disk, memory pressure, last cleanup. Read-only |
| `scan [--all]` | Cache inventory with eligibility. Read-only |
| `projects [--folder PATH] [--all]` | Build output in saved project folders. Read-only |
| `projects add PATH` / `projects remove PATH` | Saves or forgets a project folder in the app's settings. Refuses while the app is open |
| `large-files [--min-size SIZE] [--root NAME] [--installers]` | Large files under the app's roots. Read-only |
| `apps` | Installed apps by size. Read-only |
| `docker` | Docker engine status and data. Read-only |
| `worktrees [--folder PATH]` | Linked Git worktrees in the project folders and whether each is finished (#505). Read-only |
| `clean` | Preview of the one-button Clean set. Read-only |
| `clean --yes [--only REF] [--skip REF]` | Permanently removes the previewed set |
| `ports` | Development listeners. Read-only; stopping stays in the app |
| `ai [--refresh]` | AI tool usage, derived data only |
| `history` | Completed cleanups as counts |
| `doctor` | `Neati --doctor` |
| `skill print` / `skill install` | The agent skill |
| `sessions` | Claude Code and Codex sessions by project. Read-only |
| `sessions export --out FILE [--only KEY,…] [--allow-secrets]` | Writes one new bundle file. Never replaces FILE |
| `sessions import FILE [--remap OLD=NEW]` | Verifies a bundle and adds its sessions. Never replaces a file |

`--json` prints one object with `"schema": "neati.cli/1"`. Progress goes to
stderr, and only when stderr is a terminal and `--quiet` is absent.

The one-button set follows the person's cleanup rules in Settings (#504):
build output of recently worked-on projects can be kept, and dependency
folders of inactive projects can be included. `projects --json` items carry
`clean_inclusion` (why an item is in the set or left for review) and, when a
rule needed it, `project_last_activity`. Both fields are additive; the schema
stays `neati.cli/1`. The CLI reads the rules and never changes them.

When the worktree rule is on, `clean` also lists finished worktrees with
`"source": "worktree"` and a `worktree:` reference; `--only`/`--skip` accept
those references like any other. Removing one runs `git worktree remove`
after every fact is checked again; the branch stays.

| Exit code | Meaning |
| --- | --- |
| 0 | Success. A preview counts as success |
| 1 | Failure, or some items were not removed |
| 2 | Usage error |

## What `clean` may remove

`clean` removes only what the app's Clean button would remove.

- **Caches** are the units `select_quick_clean_safe_candidates` returns from a
  fresh scan: `AutoCleanable`, with cleanable bytes, in a category the person
  enabled. They run through `CleanupService::reviewed_quick_clean_safe`, which
  re-derives every unit from the current scan.
- **Build output** is the set `developer_artifacts::preselection` marks: idle,
  complete, verified compiler output in saved project folders (#494). It runs
  through a one-shot `RemovalPlanner::from_developer_artifacts` plan. The
  executor repeats the ownership and open-file checks right before each
  removal. When one unit contains another, only the outer one is submitted.

A dry run is the default.

- Without `--yes`, nothing is removed. The one exception is a person at a
  terminal, where stdin and stderr are TTYs, `--json` is off and they answer
  the prompt.
- An agent, a pipe or `--json` without `--yes` always gets the preview.

`--only` and `--skip` take references from the same run's scan.

- A cache reference is its stable scan item id.
- A project reference is `project:` plus 12 hex characters of the SHA-256 of
  its path. Developer-artifact ids are per scan.
- References only narrow the backend's set. A reference that is not in the set
  refuses the whole run before anything is removed. That covers a path, a
  reviewable item and a typo in `--skip`.

The CLI never accepts a path to remove. Reviewable, recent, advisory and
protected items are reported and left alone. The same goes for Large Files,
App Uninstaller, Temporary Folders, home Trash, Docker, local models, and
memory and port actions. Those keep the app's confirmation flows.

## Agent sessions (#508, #509)

`sessions` runs through its own driving port, `SessionLibrary`
(`services/agent_sessions/ports.rs`). `AgentSessionLibrary` serves it from
`agent_sessions/`, with the home folder from the injected
`PlatformEnvironment`. The window uses the same port through `SessionDesk`,
which holds one-shot previews and opens the system file dialogs; the window
never submits a path.

- **Inventory.** Claude Code `~/.claude/projects/<folder>/<id>.jsonl` (plus
  the `<id>/` side folder of subagents and tool results) and Codex
  `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` and
  `~/.codex/archived_sessions`. Links are never followed. At most 5,000
  sessions per tool are read, newest first. Each file gives 64 KiB of head,
  growing to 1 MiB until a typed prompt is found, and 64 KiB of tail, within
  a 512 MiB budget for the whole inventory. A whole transcript is never
  parsed. The title is the first typed prompt, capped at 120 characters.
  It is shown locally and never logged. A project folder that is gone is
  flagged. A network location (`/net`, `/Network`) is never checked.
- **Export.** `--only` takes inventory keys or session ids. An unknown or
  ambiguous reference refuses the whole export. Without `--only`, every
  session is exported. Every byte is checked for likely secrets first:
  private key blocks, `sk-ant-`, `sk-`, `ghp_`, `github_pat_`, `AKIA`,
  `xoxb-`/`xoxp-`. Findings are counts per kind. With findings and no
  `--allow-secrets`, nothing is written and the command exits 1.
- **Bundle.** A zip (`.neati-sessions`) holding each file byte for byte under
  `<tool>/<path relative to the tool folder>`, plus `manifest.json`: schema
  `neati.sessions/1`, the neati version, and for each session the tool, id,
  original `cwd`, and each file's relative path, size and SHA-256. No machine
  name or account is recorded. The file is written beside the destination
  and moved into place without replacing anything.
- **Import.** The manifest and every checksum are verified before anything is
  written. Absolute paths, `..`, links, encrypted or unlisted entries and paths
  outside the tool's session layout are refused. Files go to this machine's
  session folders with the original layout, never through a link and never
  over an existing file; a collision is kept and reported.
  `--remap OLD=NEW` applies only when OLD is missing here and NEW exists. It
  rewrites every `cwd` field that is OLD or inside it, and nothing else. A
  Claude Code session also moves to the folder named after NEW: every
  character other than an ASCII letter or digit becomes `-`. That rule was
  checked against real folder names. A path that would give a folder name
  over 200 characters is refused, because that rule is not verified.

## Settings

The CLI reads `settings.json` from Tauri's configuration directory
(`~/Library/Application Support/com.neati.desktop` on macOS).

- The one write is `projects add|remove`. It refuses while another neati
  process is running, because the app keeps its own copy and would overwrite
  the change.
- An added folder must be an existing directory and pass
  `validate_saved_project_folders`.
- Every other setting is written back exactly as it was read, atomically.

- A missing file means defaults, as on a fresh install.
- A file that does not parse refuses the run. Defaults would drop the person's
  exclusions, and the app's repair path rewrites a file the app owns.

`--folder` adds project folders for one run only. They are validated with
`validate_saved_project_folders` and then by workspace registration, as saved
folders are.

## The agent skill

`skills/neati/SKILL.md` is compiled into the binary by
`services::command_line`, which both interfaces use.

**Settings → Command line and agents** in the app:

- shows whether `neati` resolves to this app on the stated PATH;
- offers a link command to copy (the app never edits a PATH folder);
- installs or updates the skill for Claude Code and Codex. The window never
  replaces a file neati did not write.

- `neati skill install` writes it to `~/.claude/skills/neati/SKILL.md`.
- `--target codex` writes to `~/.codex/skills/neati/SKILL.md`, and `--dir PATH`
  writes to `PATH/neati/SKILL.md`.
- It replaces only a file carrying the `neati-skill:` marker unless `--force`
  is given.
- It refuses when the skill folder or the file is a link.

The skill teaches a preview-first workflow:

1. Run `clean --json`.
2. Show the user the list and say the items are permanently removed.
3. Wait for an explicit yes.
4. Run `clean --yes --only <approved refs> --json`.

It also tells agents never to remove neati's paths themselves.

## Verification

Each layer is tested at its own boundary.

- **Use case** (`services/one_button_clean/tests.rs`) runs against fake driven
  ports.
  - The preview is exactly the backend set, and the units left for review are
    only reported.
  - An unknown reference refuses before any port removal.
  - Execution submits only the chosen ids, each with its own scan.
  - Failures come back named by reference.
  - A source left out is never scanned.
- **Adapter** (`cli/tests.rs`) runs against a fake `CleanUseCase`.
  - `--json` without `--yes` never executes or prompts.
  - Without a person or `--yes`, nothing executes. A "no" executes nothing,
    and a "yes" executes once.
  - `--dry-run` never asks, and a failed unit exits 1.
- **Composed** (`tests/one_button_clean_tests.rs`) runs through
  `application_ports` and the real services in a fixture home.
  - A preview removes nothing.
  - `--only` removes exactly one unit; the neighbouring caches and the
    browser's cookies stay.
  - A path or an unknown reference refuses.
- **Boundary**: `tests/port_boundary_tests.rs`.
- **Unit**: `selection.rs`, `args.rs`, `context.rs` and `skill.rs` test
  narrowing, the grammar, settings reading and skill installation.
