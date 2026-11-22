<script lang="ts">
  import { onMount } from 'svelte';
  import type { DiagnosticsSnapshot, EnvironmentReport } from '../../models/types';
  import { settingsStore } from '../../stores/settings.svelte';
  import { frontendErrorStore } from '../../stores/frontendErrors.svelte';
  import { platformContextStore } from '../../stores/platformContext.svelte';
  import { describeError } from '../../utils/errors';
  import { LOG_DIRECTORY_FALLBACK } from '../../utils/platformCopy';
  import { tauriGetDiagnostics, tauriOpenLogsFolder, tauriRunEnvironmentSelfCheck } from '../../utils/tauri';
  import Button from '../Button.svelte';
  import Card from '../Card.svelte';
  import Badge from '../Badge.svelte';
  import ToolDetectionDisclosure from '../ai-activity/ToolDetectionDisclosure.svelte';
  import { AlertTriangle, FileText, FolderOpen, ShieldCheck } from '@lucide/svelte';

  /** Where a report is filed; `null` hides the link. */
  let { issueUrl = null }: { issueUrl?: string | null } = $props();

  let diagnosticsData = $state<DiagnosticsSnapshot | null>(null);
  let copiedDiagnostics = $state(false);
  // The same self-check the `--doctor` command line runs, so a user who cannot
  // open a terminal can still see which invariant failed on their machine.
  let selfCheck = $state<EnvironmentReport | null>(null);
  let selfCheckRunning = $state(false);
  let selfCheckError = $state<string | null>(null);

  onMount(() => { void loadDiagnostics(); });

  async function loadDiagnostics() {
    try {
      diagnosticsData = await tauriGetDiagnostics();
    } catch {
      // preview mode fallback
    }
  }

  async function handleOpenLogs() {
    try {
      await tauriOpenLogsFolder();
    } catch (e) {
      settingsStore.error = describeError(e, 'Failed to open logs folder');
    }
  }

  async function handleExportDiagnostics() {
    try {
      diagnosticsData = await tauriGetDiagnostics();
      if (typeof navigator !== 'undefined' && navigator.clipboard) {
        await navigator.clipboard.writeText(JSON.stringify(diagnosticsData, null, 2));
        copiedDiagnostics = true;
        setTimeout(() => {
          copiedDiagnostics = false;
        }, 2500);
      }
    } catch (e) {
      settingsStore.error = describeError(e, 'Failed to export diagnostics');
    }
  }

  async function handleRunSelfCheck() {
    selfCheckRunning = true;
    selfCheckError = null;
    try {
      selfCheck = await tauriRunEnvironmentSelfCheck();
    } catch (e) {
      selfCheck = null;
      selfCheckError = describeError(e, 'Environment self-check did not complete');
    } finally {
      selfCheckRunning = false;
    }
  }

</script>

<!-- Diagnostics & Logs -->
<div class="space-y-3">
  <h3 class="text-meta font-semibold text-muted-foreground tracking-normal">
    Diagnostics and logs
  </h3>
  <ToolDetectionDisclosure />
  <Card class="p-4 bg-card space-y-4">
    <div class="space-y-1">
      <div class="text-meta font-medium text-foreground">Local System & Error Logs</div>
      <p class="text-meta text-muted-foreground leading-relaxed">
        neati keeps zero telemetry and never transmits analytics or secrets. Error and subprocess failure logs are stored locally on your machine at <code class="font-mono text-caption bg-secondary/80 px-1 py-0.5 rounded">{platformContextStore.logDirectory ?? LOG_DIRECTORY_FALLBACK}</code>.
      </p>
    </div>

    {#if diagnosticsData?.settings_corrupt_recovered}
      <div class="flex items-center gap-2 rounded-lg border status-chip-warning px-3 py-2 text-meta text-warning">
        <AlertTriangle size={14} class="shrink-0" />
        <span>A damaged configuration file was detected, safely backed up, and reset to defaults.</span>
      </div>
    {/if}

    <div class="flex flex-wrap gap-2 pt-1">
      <Button variant="secondary" size="sm" onclick={handleOpenLogs}>
        <FolderOpen size={14} />
        <span>Open logs</span>
      </Button>
      <Button variant="secondary" size="sm" onclick={handleExportDiagnostics}>
        <FileText size={14} />
        <span>{copiedDiagnostics ? 'Diagnostics copied' : 'Copy diagnostics'}</span>
      </Button>
      <Button
        variant="secondary"
        size="sm"
        onclick={handleRunSelfCheck}
        disabled={selfCheckRunning}
      >
        <ShieldCheck size={14} />
        <span>{selfCheckRunning ? 'Checking environment…' : 'Check environment'}</span>
      </Button>
    </div>
    {#if issueUrl}
      <p class="text-meta text-muted-foreground">
        Found a problem? Copy diagnostics, then
        <a class="text-primary underline underline-offset-2" href={issueUrl} target="_blank" rel="noreferrer noopener" title="Opens a new GitHub issue in your browser">open an issue</a>
        and paste it. Diagnostics contain no file names from your cleanups; review them before posting.
      </p>
    {/if}

    <div class="space-y-2 border-t border-border/60 pt-3">
      <div class="text-meta font-medium text-foreground">Uncaught Frontend Errors</div>
      {#if frontendErrorStore.entries.length === 0}
        <p class="text-meta text-muted-foreground">No uncaught frontend errors.</p>
      {:else}
        <ul class="space-y-1.5">
          {#each frontendErrorStore.entries as entry (entry.id)}
            <li
              class="rounded-lg border status-panel-destructive px-3 py-2 text-meta"
              role="alert"
            >
              <div class="flex items-center gap-2 text-destructive">
                <AlertTriangle size={14} class="shrink-0" />
                <span class="font-medium">
                  {entry.kind === 'rejection' ? 'Unhandled rejection' : 'Uncaught error'}
                </span>
                <span class="font-mono text-caption text-muted-foreground">{entry.at}</span>
              </div>
              <p class="mt-1 break-words font-mono text-caption text-foreground/90">
                {entry.message}
              </p>
            </li>
          {/each}
        </ul>
      {/if}
    </div>

    {#if selfCheckError}
      <div
        class="mt-3 flex items-start gap-2 rounded-lg border status-chip-destructive px-3 py-2 text-meta text-destructive"
        role="alert"
      >
        <AlertTriangle size={14} class="mt-0.5 shrink-0" />
        <span>{selfCheckError}</span>
      </div>
    {/if}

    {#if selfCheck}
      <div class="mt-3 space-y-2">
        {#if selfCheck.failures > 0}
          <div
            class="flex items-center gap-2 rounded-lg border status-chip-destructive px-3 py-2 text-meta text-destructive"
            role="alert"
          >
            <AlertTriangle size={14} class="shrink-0" />
            <span>
              {selfCheck.failures} self-check
              {selfCheck.failures === 1 ? 'invariant failed' : 'invariants failed'} on this
              machine. Report it with the Windows bug report template.
            </span>
          </div>
        {:else}
          <div
            class="flex items-center gap-2 rounded-lg border status-chip-success px-3 py-2 text-meta text-success"
          >
            <ShieldCheck size={14} class="shrink-0" />
            <span>All {selfCheck.checks.length} environment self-checks passed.</span>
          </div>
        {/if}

        <div class="rounded-lg bg-secondary/40 border border-border/40 p-3 text-meta font-mono text-muted-foreground space-y-1">
          <div class="text-foreground font-semibold">Environment fingerprint</div>
          {#each selfCheck.fingerprint as line}
            <div class="truncate">{line}</div>
          {/each}
        </div>

        <div
          class="rounded-lg bg-secondary/40 border border-border/40 p-3 text-meta space-y-1"
          aria-label="Environment self-check results"
        >
          {#each selfCheck.checks as row}
            <div class="flex items-start gap-2">
              <span
                class={row.outcome === 'pass' ? 'text-success font-semibold' : 'text-destructive font-semibold'}
              >
                {row.outcome === 'pass' ? 'PASS' : 'FAIL'}
              </span>
              <span class="font-mono text-foreground">{row.name}</span>
              <span class="text-muted-foreground truncate">{row.detail}</span>
            </div>
          {/each}
        </div>
      </div>
    {/if}

    {#if diagnosticsData}
      <!-- svelte-ignore a11y_no_noninteractive_tabindex (keyboard users must be able to scroll the diagnostics region) -->
      <div
        role="region"
        aria-label="Application diagnostics"
        tabindex="0"
        class="mt-3 rounded-lg bg-secondary/40 border border-border/40 p-3 text-meta font-mono text-muted-foreground space-y-1 overflow-x-auto max-h-48 overflow-y-auto scroll-stable focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <div>
          <span class="text-foreground font-semibold">neati:</span>
          {diagnosticsData.app_version} ({diagnosticsData.arch}{diagnosticsData.emulated ? ', emulated' : ''})
        </div>
        <div><span class="text-foreground font-semibold">OS:</span> {diagnosticsData.os_version}</div>
        {#if diagnosticsData.native_arch}
          <div><span class="text-foreground font-semibold">Native Arch:</span> {diagnosticsData.native_arch}</div>
        {/if}
        {#if diagnosticsData.webview_version}
          <div><span class="text-foreground font-semibold">WebView:</span> {diagnosticsData.webview_version}</div>
        {/if}
        {#if diagnosticsData.elevated !== null}
          <div><span class="text-foreground font-semibold">Elevated:</span> {diagnosticsData.elevated ? 'yes' : 'no'}</div>
        {/if}
        {#if diagnosticsData.locale}
          <div><span class="text-foreground font-semibold">Locale:</span> {diagnosticsData.locale}</div>
        {/if}
        <div><span class="text-foreground font-semibold">Log:</span> {diagnosticsData.log_path}</div>
        {#if diagnosticsData.log_failure}
          <div class="pt-2 text-destructive font-semibold">Diagnostics encountered a write failure:</div>
          <div class="text-destructive/80">{diagnosticsData.log_failure}</div>
        {/if}
        {#if diagnosticsData.recent_errors.length > 0}
          <div class="pt-2 text-destructive font-semibold">Recent Errors ({diagnosticsData.recent_errors.length}):</div>
          {#each diagnosticsData.recent_errors as err}
            <div class="text-destructive/80 truncate">{err}</div>
          {/each}
        {:else}
          <!-- An empty list is absence of data, not evidence of health. -->
          <div class="pt-1 text-muted-foreground">No recent errors in the log file.</div>
        {/if}
      </div>
    {/if}
  </Card>
</div>
