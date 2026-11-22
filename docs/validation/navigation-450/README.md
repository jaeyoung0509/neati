# Navigation consolidation (#450)

Browser preview (`pnpm dev`, mocked IPC), 1100 × 760, light theme.

| File | Shows |
|---|---|
| `clean-section.jpg` | Sidebar with Home / Clean / System / AI / Settings; Clean tabs (Caches, Projects, Large Files, Apps, Containers, Models) |
| `system-section.jpg` | System tabs (Performance, Ports, Keep Awake). Captured before the page-header Beta badge was removed |
| `ai-section.jpg` | AI tabs (Activity, Controls) with Beta only on the tabs |

Not verified: the collapsed sidebar, dark theme, the native window, and
navigation requested from the tray or Quick Panel in the native app (route ids
are unchanged and covered by unit tests).
