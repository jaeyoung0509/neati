# Final four-batch UI verification

Recorded on **October 2, 2026**, with neati **0.3.104**. Native observations used
macOS **27.0.1 (26A434)** on Apple Silicon, WebView **22625.1.29.11.28**.
The installed `/Applications/neati.app` was not replaced. No user files were cleaned.

## Browser evidence

The fixtures mount production Svelte components and CSS with synthetic inventories
and mock IPC. They do not establish native compositing or OS permission behavior.

- Historical unchanged-layout checks: 66 dialog interactions and 78 main-route,
  theme and size cases; 2,754 sampled text contrasts had no failures.
- Integrated 0.3.104 at `996c164`: 26 full main-route keyboard traversals across
  13 routes, light/dark, 800 × 560, 125% text and long English/Korean labels;
  22 fresh dialog checks at 800 px; 156 Quick Panel cases plus lifecycle,
  constrained footer, obsolete activation and Escape checks.
- The three images in this directory are browser captures from the earlier
  unchanged visual implementation. They show cleanup review, Keep Awake and
  the constrained Quick Panel, not native glass.
- Framework artifact mounted checks confirmed empty initial selection, disabled
  custom output, and select-all choosing one whole parent instead of overlapping
  children. Individual child toggling and exact preview were not completed in
  the mounted browser because fixture startup/control failed. Their store/plan
  regression tests are separate evidence. Browser deletion stays unavailable.
- The original six progress/Stop fixture checks returned empty, exhausted results.
  They did **not** exercise the native nonempty stopped result described below.

## Native observations and owner check

The exact preserved 0.3.104 `996c164` application was launched. A native scan
completed with partial coverage and responsive progress; no superseded-publication
error was seen in that run. Its `--doctor` reported 14/14 checks passing.

The owner manually opened the native Quick Panel and reported all three requested
checks passing: bounds/footer stay stable while numbers and gauges arrive after
20–30 seconds; three hide/reopen cycles do not jump; Escape closes the panel and
Open neati stays reachable and opens the dashboard. This is an **owner-reported
manual result**, not a captured video or measured bounds trace. The configured
native width is 400 logical px; 320/360 px results are browser evidence only.

Stopping a native scan retained three AutoCleanable rows and a 376 KB estimate.
Cleanup was correctly disabled, but the old UI restored three selections and
said Ready now. The follow-up in this PR preserves stopped/paused measurements
as **Checked estimate**, keeps zero selections and requires completion before
cleanup. Completed partial scans still permit their verified items. The regression
fixture now retains the same nonempty stopped shape and positive measured bytes.

## Settings and remaining native acceptance

The owner requested restoration. Reduce Motion and Reduce Transparency were
both restored **off** and verified in System Settings. neati theme was restored
**System**. The existing manual indefinite Keep Awake session, with display sleep
allowed and zero enabled automatic rules, was restored. Full Disk Access,
VoiceOver, signing and installed application files were not changed.

Still unverified: controlled colorful/neutral-dark compositing comparison;
VoiceOver output; animation/compositing behavior under reduced settings;
physical bounds/video and another display/DPI/work area; native permission
grant/revoke/relaunch/upgrade transitions. Keep #362, #376, #379 and #389 native
acceptance distinct from local tests and owner checks. Windows desktop acceptance
remains #380; no Windows/Linux runtime parity is claimed here.
