# Temporary review and framework children

Version 0.3.99, October 1, 2026, macOS 27.0.1 (26A434).
Chromium browser fixtures; these images do not demonstrate native Trash or Windows behavior.

- `temporary-800x560.png`: minimum window, empty selection and qualified temporary total.
- `temporary-confirm-800x560.png`: exact path, distinct uncertainty/loss consent and manual restoration notice.
- `temporary-dark-960x660.png` and `temporary-result-dark-960x660.png`: dark appearance, preview-only result.
- `framework-children-dark-1280x800.png`: generated children are selectable; deployment parents remain observation-only.

Checked light/dark appearance, reduced motion, no horizontal overflow, disabled active/access-blocked units, both required consents, and Escape returning focus to Review selected. Browser preview changed no files. Rust native safety tests use disposable fixtures and a fixture-only Trash port.

Final A/B/C integration includes B `b676368` and develop `b4c4d89`. Workspace/all-target Rust tests: 1,389 passed, 8 ignored, zero failed; check, format/Clippy, architecture and generated binding/golden checks passed. Frontend typecheck: zero errors/warnings; 497 tests, production build, synchronized-version and 61-icon checks passed. The import-only Windows fix also passed local format/Clippy.

Locked `just build-fast` produced version/build 0.3.99 with the exact source icon and all 13 current JS/CSS assets' encoded bytes embedded. The installed/running application was not replaced. Windows native behavior and CI results remain separate from these local macOS checks.
