# AI tool profiles (#451)

`settings-ai-tool-profiles.jpg`: Settings → AI tool profiles in the browser preview (`pnpm dev`, mocked IPC), after adding the suggested Claude profile. The Codex suggestion is still offered.

Not verified in the native app: adding a real `~/.claude-work`, the refusal messages for an invalid folder, and the profile item appearing in Clean after a rescan. The backend path is covered by `a_scan_measures_a_confirmed_profile_as_observation_only` and the two settings-save tests.
