# Design system consolidation — 0.3.111 (#430)

Browser preview (`pnpm dev`, mocked IPC), 1100 × 760 viewport, light theme.

| File | Shows |
|---|---|
| `dialog-empty-trash.jpg` | Shared `Dialog` shell (component family): 20 px radius, `backdrop:bg-foreground/30` |
| `dialog-dev-server-release.jpg` | A view dialog migrated from the old `rounded-xl` / `backdrop:bg-background/80` shell; initial focus on **Cancel** |
| `status-chip-destructive-notice.jpg` | `InlineNotice` `destructive` on `status-chip-destructive` |
| `keep-awake-type-scale.jpg` | Keep Awake after `text-xs` → `text-meta` and `status-*` migration |

Interaction checks run in the preview:

- Escape on the Trash dialog closed it and returned focus to **Review Trash**.
- The Dev Servers release dialog opened with focus on `#release-cancel`.

Not verified here: the native Tauri window (glass material, dark mode), and
Quick Panel dialogs at the 400 px panel width.
