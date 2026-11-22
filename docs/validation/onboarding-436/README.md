# Onboarding and launch at login (#436)

Browser preview (`pnpm dev`, mocked IPC), 1100 × 760, light theme.

| File | Shows |
|---|---|
| `step-1-welcome.jpg` | Step 1: what neati cleans |
| `step-2-permanent.jpg` | Step 2: cleaning is permanent removal |
| `step-3-optional-setup.jpg` | Step 3: Full Disk Access and start-at-login (preview reports the switch as unavailable) |
| `settings-general.jpg` | Settings → General: start-at-login switch and **Show guide** |

Checked in the preview: **Show guide** reopens the guide, Next/Back move between
steps, Tab stays inside the dialog, and **Done** closes it and returns focus to
**Show guide**.

Not verified: the guide on a real first launch of the native app, and the
LaunchAgent actually starting neati at the next login (needs an installed copy
and a logout). The adapter is covered by unit tests against a temporary
directory.
