---
name: neati
description: Free disk space on a Mac with the neati CLI. It finds regenerable developer caches and idle project build output (Cargo target, Gradle, Xcode DerivedData, npm/pnpm/Homebrew caches and more) and permanently removes them only after the user approves. It also reports disk, memory, local dev servers and AI tool usage. Use when the user says their disk is full or nearly full, asks what is using space, wants caches or build folders cleaned, or asks which dev servers are running or how much Claude/Codex/Gemini quota is left.
---
<!-- neati-skill: written by `neati skill install`; running it again replaces this file. -->

# neati

neati is a macOS storage cleaner for developers. Its CLI uses the same scan,
plan and removal code as the app, and it only ever removes the set the app's
one-button **Clean** would. That set is idle, regenerable caches and idle,
verified compiler build output in the user's saved project folders. If the
user turned on the dependency rule in neati's Settings, it also includes the
dependency folders (`node_modules`, Python environments, `vendor`) of projects
they have not worked on for a while, when a lockfile can reinstall them. Each
`neati projects --all --json` item says why it is or is not included
(`clean_inclusion`). With the worktree rule on, the set also includes finished
Git worktrees (`"source": "worktree"`): everything committed and on a branch,
nothing using them. Git removes them and keeps the branch. Name them as
worktrees when you show the preview. Change those rules only in the app; never edit neati's
settings file.

## Before you start

- Check that the CLI is there with `neati version`. If it is missing:
  - The app may be installed without the command: `/Applications/neati.app/Contents/MacOS/Neati version`.
  - Otherwise ask the user to install it with `brew install --cask jaeyoung0509/neati/neati`. Do not install it yourself without asking.
- Always pass `--json` and read stdout. Progress goes to stderr; ignore it.
- macOS may show a permission prompt the first time neati reads another app's
  data. Tell the user to answer it; do not retry in a loop.

## Commands

| Command | What it does |
| --- | --- |
| `neati status --json` | Free disk space, memory pressure, last cleanup |
| `neati clean --json` | **Preview** of what Clean would remove. Nothing is removed |
| `neati clean --yes --only REF,REF --json` | Permanently removes exactly those references |
| `neati scan --json` | Cache inventory; `--all` adds items that need review and why |
| `neati projects --json` | Build output in saved project folders; `--folder PATH` adds one for this run |
| `neati worktrees --json` | Git worktrees in saved project folders and whether each is finished (read-only) |
| `neati projects add PATH --json` | Saves a project folder for future runs (refuses while the neati app is open) |
| `neati large-files --json` | Large files in Downloads, Desktop, Documents, Movies (`--min-size 1G`, `--root downloads`), read-only |
| `neati apps --json` | Installed apps by size, read-only |
| `neati docker --json` | Docker engine status, images, containers, volumes, read-only |
| `neati ports --json` | Development servers listening (port, pid, project) |
| `neati ai --json` | AI tool usage windows and token counts |
| `neati history --json` | Past cleanups |

Every JSON object has `"schema": "neati.cli/1"` and `"command"`. If a command
fails, it prints `{"error": "..."}` and exits 1. A usage error exits 2.

## Freeing space: the required workflow

1. Run `neati status --json`. Note `disk.available_bytes`.
2. Run `neati clean --json`. It is a dry run (`"dry_run": true`) and lists
   `items[]`, each with `ref`, `name`, `kind`, `path`, `bytes` and `source`
   (`cache` or `project`), plus `total_bytes`.
3. Show the user the list and the total in plain words. Group small items if
   there are many. Say that these are **permanently removed** and are rebuilt
   or re-downloaded when needed. Mention `review_in_app` if it is non-zero.
4. Wait for an explicit yes in this conversation. If they want to keep
   something, leave its `ref` out.
5. Run `neati clean --yes --only <the approved refs> --json`. Always pass the
   approved refs with `--only`, so a cache that appeared after the preview is
   not removed without approval. If any ref is no longer in the set, the run
   refuses and removes nothing. Then preview again.
6. Report `result.reclaimed_bytes` and any `result.failed[]` messages, then
   run `neati status --json` again if the user wants the new free space.

If `project_folders.saved` is empty, the user has no saved project folder, so
build output was not checked. Ask which folder holds their code, then pass it
with `--folder ~/code` on both the preview and the `--yes` run. To remember it,
`neati projects add ~/code` saves it, but only while the neati app is closed.

## Rules

- Never pass `--yes` without the user's explicit approval of that preview in
  this conversation. Approval does not carry over to a later run.
- Never delete paths from neati's output yourself with `rm`, `trash` or
  `find -delete`. neati checks ownership, open files and links right before
  removal; a manual delete skips all of that.
- Removing items that need review, large files, apps, Docker data, local
  models and the Trash stays in the app. The CLI can list large files, apps
  and Docker data so you can point the user at them; never delete them
  yourself.
- Do not run `clean` repeatedly to "make sure". Preview once, remove once.
- neati only lists dev servers (`ports`); it does not stop them from the CLI.
