# UI refinement (#462)

Browser preview (`pnpm dev`, mocked IPC), 960 × 660 (the default window size), light theme.

| File | Shows |
|---|---|
| `before-clean-caches.jpg` | Clean → Caches before: tall summary card with an empty rule area, list below the fold, title "Cleanup" under a "Caches" tab |
| `after-clean-caches.jpg` | After: shorter card, two categories visible without scrolling, title matches the tab, outlined primary action |
| `after-home.jpg` | Home after switching from a scrolled Clean view: starts at the top; one primary action and a quiet secondary |

Checked in the preview: after scrolling Clean and selecting Home, the content
column's `scrollTop` is 0.

Not verified: dark theme, the collapsed sidebar, the native window and its
glass material, and the Quick Panel copy change ("Open Clean") in the panel
itself.
