<script lang="ts">
  import { onMount } from 'svelte';
  import { Archive, CircleStop, Download, FolderSearch, FolderX, MessagesSquare, RotateCw, Upload, X } from '@lucide/svelte';
  import { agentSessionsStore as sessions } from '../../lib/stores/agentSessions.svelte';
  import { awaitingFirstAnswer } from '../../lib/utils/loadPhase';
  import { formatTimeAgo } from '../../lib/utils/format';
  import {
    SESSION_TOOL_LABELS,
    bundleSummary,
    findingLines,
    folderStateLabel,
    sessionIssueLabel,
    sessionLabel,
  } from '../../lib/utils/agentSessions';
  import Badge from '../../lib/components/Badge.svelte';
  import Button from '../../lib/components/Button.svelte';
  import ByteValue from '../../lib/components/ByteValue.svelte';
  import Checkbox from '../../lib/components/Checkbox.svelte';
  import Dialog from '../../lib/components/Dialog.svelte';
  import EmptyState from '../../lib/components/EmptyState.svelte';
  import InlineNotice from '../../lib/components/InlineNotice.svelte';
  import LoadingActionContent from '../../lib/components/LoadingActionContent.svelte';
  import LoadingState from '../../lib/components/LoadingState.svelte';
  import PageHeader from '../../lib/components/PageHeader.svelte';

  /**
   * Agent sessions (#508, #509): Claude Code and Codex sessions by project,
   * read-only, with bundles to carry them to another Mac. neati never changes
   * or removes a session file here.
   */
  let acknowledged = $state(false);

  onMount(() => {
    void sessions.load();
  });

  let inventory = $derived(sessions.inventory);
  let projects = $derived(inventory?.projects ?? []);
  let reading = $derived(sessions.reading);
  let busy = $derived(sessions.exportStep === 'preparing' || sessions.exportStep === 'writing' || sessions.importStep === 'importing');
  let selectedCount = $derived(sessions.selected.length);
  let anyTool = $derived(inventory?.tools.some((tool) => tool.present) ?? false);
  let findings = $derived(sessions.exportPreview?.findings ?? []);
  let missingProjects = $derived(sessions.bundle?.projects.filter((project) => project.folder === 'missing') ?? []);
  let keptFiles = $derived(sessions.bundle?.sessions.reduce((sum, entry) => sum + entry.existing_files, 0) ?? 0);

  function projectKeys(index: number): string[] {
    return projects[index]?.sessions.map((entry) => entry.key) ?? [];
  }

  function projectSelection(index: number): { checked: boolean; indeterminate: boolean } {
    const keys = projectKeys(index);
    const chosen = keys.filter((key) => sessions.isSelected(key)).length;
    return { checked: chosen > 0 && chosen === keys.length, indeterminate: chosen > 0 && chosen < keys.length };
  }

  function openExport() {
    acknowledged = false;
    void sessions.prepareExport();
  }
</script>

<div class="space-y-6">
  <PageHeader
    title="Sessions"
    subtitle="Claude Code and Codex sessions on this Mac, grouped by project. neati reads them and never changes or removes them."
    icon={MessagesSquare}
  >
    {#snippet actions()}
      <Button
        variant="ghost"
        size="sm"
        class="gap-1.5 text-meta"
        disabled={reading}
        onclick={() => void sessions.load()}
        aria-label="Read sessions again"
        title="Read sessions again"
      >
        <LoadingActionContent busy={reading} busyLabel="Reading…">
          <RotateCw size={13} aria-hidden="true" /><span>Refresh</span>
        </LoadingActionContent>
      </Button>
      <Button
        variant="outline"
        size="sm"
        class="gap-1.5 text-meta"
        disabled={sessions.importStep !== 'idle' || busy}
        onclick={() => void sessions.openBundle()}
      >
        <LoadingActionContent busy={sessions.importStep === 'opening'} busyLabel="Verifying…">
          <Upload size={13} aria-hidden="true" /><span>Import bundle…</span>
        </LoadingActionContent>
      </Button>
      <Button
        size="sm"
        class="gap-1.5 text-meta"
        disabled={selectedCount === 0 || sessions.exportStep !== 'idle' || busy}
        title={selectedCount === 0 ? 'Select sessions to export' : undefined}
        onclick={openExport}
      >
        <LoadingActionContent busy={sessions.exportStep === 'preparing'} busyLabel="Checking…">
          <Download size={13} aria-hidden="true" /><span>Export {selectedCount > 0 ? `${selectedCount} ` : ''}selected…</span>
        </LoadingActionContent>
      </Button>
    {/snippet}
  </PageHeader>

  {#if inventory && anyTool}
    <p class="flex flex-wrap gap-x-4 gap-y-1 text-meta text-muted-foreground" aria-label="Totals per tool">
      {#each inventory.tools as tool (tool.tool)}
        <span>
          <span class="font-medium text-foreground">{SESSION_TOOL_LABELS[tool.tool]}</span>
          {#if tool.present}
            · {tool.sessions} {tool.sessions === 1 ? 'session' : 'sessions'} · <ByteValue bytes={tool.bytes} />
          {:else}
            · not found on this Mac
          {/if}
        </span>
      {/each}
    </p>
  {/if}

  {#if sessions.exportStep === 'preparing'}
    <div class="flex items-center gap-3 text-meta text-muted-foreground" role="status">
      <span>Checking the selected sessions for likely secrets…</span>
      <Button variant="ghost" size="xs" class="gap-1" onclick={() => void sessions.stop()}>
        <CircleStop size={12} aria-hidden="true" /><span>Stop</span>
      </Button>
    </div>
  {/if}

  {#if sessions.exportResult}
    <InlineNotice
      variant="success"
      title="Bundle saved"
      message={`${bundleSummary(sessions.exportResult.sessions, sessions.exportResult.files, sessions.exportResult.bytes)} written to ${sessions.exportResult.path}.`}
      onDismiss={() => (sessions.exportResult = null)}
    />
  {/if}
  {#if sessions.importResult}
    <InlineNotice
      variant="success"
      title="Sessions imported"
      message={`${sessions.importResult.written_files} files written${sessions.importResult.kept_existing.length > 0 ? `; ${sessions.importResult.kept_existing.length} existing files were kept, not replaced` : ''}${sessions.importResult.rewritten_files > 0 ? `; the project folder was updated in ${sessions.importResult.rewritten_files} files` : ''}.`}
      onDismiss={() => (sessions.importResult = null)}
    />
  {/if}
  {#if sessions.exportError && sessions.exportStep === 'idle'}
    <InlineNotice variant="destructive" title="Export stopped" message={sessions.exportError} onDismiss={() => (sessions.exportError = null)} />
  {/if}
  {#if sessions.importError && sessions.importStep === 'idle'}
    <InlineNotice variant="destructive" title="Import stopped" message={sessions.importError} onDismiss={() => (sessions.importError = null)} />
  {/if}

  {#if sessions.error && sessions.phase === 'ready'}
    <InlineNotice variant="destructive" title="Sessions could not be read again" message={sessions.error} actionLabel="Retry" onAction={() => void sessions.load()} />
  {/if}
  {#if inventory && inventory.incomplete_reasons.length > 0}
    <InlineNotice variant="warning" title={inventory.truncated ? 'Some sessions were not read' : 'Some sessions are incomplete'} message={inventory.incomplete_reasons.join(' ')} />
  {/if}

  {#if awaitingFirstAnswer(sessions.phase)}
    <LoadingState message="Reading agent sessions…" detail="Only the first and last part of each session file is read." />
  {:else if sessions.phase === 'error'}
    <EmptyState
      icon={MessagesSquare}
      title="Sessions could not be read"
      description={sessions.error ?? undefined}
      actionLabel="Retry"
      onAction={() => void sessions.load()}
    />
  {:else if !anyTool}
    <EmptyState
      icon={MessagesSquare}
      title="No session folders found"
      description="Claude Code keeps sessions in ~/.claude/projects and Codex in ~/.codex/sessions. Neither folder exists on this Mac."
    />
  {:else if projects.length === 0}
    <EmptyState icon={MessagesSquare} title="No sessions yet" description="Sessions appear here after Claude Code or Codex records one." />
  {:else}
    <div class="space-y-4">
      {#each projects as project, index (project.key)}
        {@const selection = projectSelection(index)}
        {@const folderLabel = folderStateLabel(project.folder)}
        <section class="rounded-xl border border-border bg-card" aria-label={project.display_name}>
          <header class="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-border px-4 py-3">
            <Checkbox
              checked={selection.checked}
              indeterminate={selection.indeterminate}
              ariaLabel={`Select every session in ${project.display_name}`}
              title={`Select every session in ${project.display_name}`}
              onchange={(on) => sessions.setSelected(projectKeys(index), on)}
            />
            <div class="min-w-0 flex-1">
              <div class="flex flex-wrap items-center gap-2">
                <h2 class="text-body font-semibold text-foreground break-words">{project.display_name}</h2>
                {#if project.folder === 'missing'}
                  <Badge variant="warning" class="gap-1"><FolderX size={11} aria-hidden="true" />{folderLabel}</Badge>
                {:else if folderLabel}
                  <Badge variant="outline">{folderLabel}</Badge>
                {/if}
              </div>
              {#if project.cwd}
                <p class="truncate font-mono text-caption text-muted-foreground" title={project.cwd}>{project.cwd}</p>
              {/if}
            </div>
            <span class="text-meta text-muted-foreground">{project.sessions.length} {project.sessions.length === 1 ? 'session' : 'sessions'}</span>
            <ByteValue bytes={project.total_bytes} class="w-20 text-right text-meta font-semibold text-foreground" />
          </header>
          <ul class="divide-y divide-border">
            {#each project.sessions as entry (entry.key)}
              {@const issue = sessionIssueLabel(entry.issue)}
              <li class="flex items-center gap-3 px-4 py-2.5">
                <Checkbox
                  checked={sessions.isSelected(entry.key)}
                  ariaLabel={`Select ${sessionLabel(entry.title, entry.session_id)}`}
                  onchange={(on) => sessions.setSelected([entry.key], on)}
                />
                <div class="min-w-0 flex-1">
                  <p class="truncate text-body text-foreground" title={entry.title ?? entry.session_id}>
                    {sessionLabel(entry.title, entry.session_id)}
                  </p>
                  <p class="flex flex-wrap items-center gap-x-2 text-caption text-muted-foreground">
                    <span>{SESSION_TOOL_LABELS[entry.tool]}</span>
                    <span>· {entry.last_activity_at ? formatTimeAgo(entry.last_activity_at) : 'date unknown'}{entry.dates_from_records ? '' : ' (file date)'}</span>
                    <span>· {entry.file_count} {entry.file_count === 1 ? 'file' : 'files'}</span>
                    {#if entry.archived}<span class="inline-flex items-center gap-1">· <Archive size={11} aria-hidden="true" />Archived</span>{/if}
                    {#if issue}<span class="text-warning">· {issue}</span>{/if}
                  </p>
                </div>
                <ByteValue bytes={entry.size_bytes} class="w-20 text-right text-meta text-foreground" />
              </li>
            {/each}
          </ul>
        </section>
      {/each}
    </div>
  {/if}

  {#if sessions.exportPreview && (sessions.exportStep === 'review' || sessions.exportStep === 'writing')}
    {@const preview = sessions.exportPreview}
    <Dialog
      onCancel={() => sessions.closeExport()}
      initialFocus="#session-export-cancel"
      aria-labelledby="session-export-title"
      aria-describedby="session-export-description"
      size="md"
      class="space-y-4"
    >
      <div>
        <h3 id="session-export-title" class="text-sm font-semibold">Export sessions to a bundle</h3>
        <p id="session-export-description" class="mt-1 text-meta leading-relaxed text-muted-foreground">
          {bundleSummary(preview.sessions, preview.files, preview.bytes)}. The bundle holds the session files exactly as they are, with a checksummed manifest. No machine name or account is recorded. neati never replaces an existing file.
        </p>
      </div>
      {#if findings.length > 0}
        <div class="status-panel-warning space-y-2 rounded-lg border p-3" role="alert">
          <p class="text-meta font-semibold text-warning">Likely secrets in these sessions</p>
          <ul class="list-disc pl-5 font-mono text-meta text-foreground">
            {#each findingLines(findings) as line (line)}<li>{line}</li>{/each}
          </ul>
          <p class="text-meta text-muted-foreground">Only counts are shown. Anyone who opens the bundle can read them.</p>
          <Checkbox
            checked={acknowledged}
            ariaLabel="Include the likely secrets in the bundle anyway"
            label="Include them in the bundle anyway"
            class="w-fit justify-start gap-2"
            onchange={(on) => (acknowledged = on)}
          />
        </div>
      {:else}
        <p class="text-meta text-muted-foreground">No likely secrets were found. Sessions can still hold other private text.</p>
      {/if}
      {#if sessions.exportError}
        <InlineNotice variant="destructive" message={sessions.exportError} />
      {/if}
      <div class="flex justify-end gap-2">
        {#if sessions.exportStep === 'writing'}
          <Button variant="ghost" size="sm" class="gap-1" onclick={() => void sessions.stop()}>
            <CircleStop size={12} aria-hidden="true" /><span>Stop</span>
          </Button>
        {/if}
        <Button id="session-export-cancel" variant="ghost" size="sm" disabled={sessions.exportStep === 'writing'} onclick={() => sessions.closeExport()}>Cancel</Button>
        <Button
          size="sm"
          disabled={sessions.exportStep === 'writing' || (findings.length > 0 && !acknowledged)}
          title={findings.length > 0 && !acknowledged ? 'Confirm the findings first' : undefined}
          onclick={() => void sessions.exportBundle(findings.length > 0 && acknowledged)}
        >
          <LoadingActionContent busy={sessions.exportStep === 'writing'} busyLabel="Writing…">
            <Download size={13} aria-hidden="true" /><span>Choose where to save…</span>
          </LoadingActionContent>
        </Button>
      </div>
    </Dialog>
  {/if}

  {#if sessions.bundle && (sessions.importStep === 'review' || sessions.importStep === 'importing')}
    {@const bundle = sessions.bundle}
    <Dialog
      onCancel={() => sessions.closeImport()}
      initialFocus="#session-import-cancel"
      aria-labelledby="session-import-title"
      aria-describedby="session-import-description"
      size="lg"
      class="space-y-4"
    >
      <div>
        <h3 id="session-import-title" class="text-sm font-semibold">Import {bundle.name}</h3>
        <p id="session-import-description" class="mt-1 text-meta leading-relaxed text-muted-foreground">
          Verified: every file matches its checksum. {bundle.sessions.length} {bundle.sessions.length === 1 ? 'session' : 'sessions'} from neati {bundle.neati_version} go into the Claude Code and Codex session folders. Existing files are kept, never replaced{keptFiles > 0 ? ` (${keptFiles} already here)` : ''}.
        </p>
      </div>
      {#if missingProjects.length > 0}
        <div class="space-y-2">
          <p class="text-meta font-semibold text-foreground">Project folders that are not on this Mac</p>
          <p class="text-meta text-muted-foreground">Choose where each project lives now, or import the sessions with the recorded folder.</p>
          <ul class="divide-y divide-border rounded-lg border border-border">
            {#each missingProjects as project (project.original_cwd)}
              {@const chosen = sessions.remaps[project.original_cwd]}
              <li class="flex flex-wrap items-center gap-2 px-3 py-2">
                <div class="min-w-0 flex-1">
                  <p class="truncate font-mono text-caption text-muted-foreground" title={project.original_cwd}>{project.original_cwd}</p>
                  {#if chosen}
                    <p class="truncate font-mono text-caption text-foreground" title={chosen}>→ {chosen}</p>
                  {/if}
                </div>
                <Button variant="outline" size="xs" class="gap-1" disabled={sessions.importStep === 'importing'} onclick={() => void sessions.chooseRemap(project.original_cwd)}>
                  <FolderSearch size={12} aria-hidden="true" /><span>{chosen ? 'Change…' : 'Choose folder…'}</span>
                </Button>
                {#if chosen}
                  <Button
                    variant="ghost"
                    size="icon"
                    class="h-7 w-7"
                    aria-label={`Keep the recorded folder for ${project.original_cwd}`}
                    title="Keep the recorded folder"
                    disabled={sessions.importStep === 'importing'}
                    onclick={() => sessions.clearRemap(project.original_cwd)}
                  >
                    <X size={12} aria-hidden="true" />
                  </Button>
                {/if}
              </li>
            {/each}
          </ul>
        </div>
      {/if}
      {#if sessions.importError}
        <InlineNotice variant="destructive" message={sessions.importError} />
      {/if}
      <div class="flex justify-end gap-2">
        {#if sessions.importStep === 'importing'}
          <Button variant="ghost" size="sm" class="gap-1" onclick={() => void sessions.stop()}>
            <CircleStop size={12} aria-hidden="true" /><span>Stop</span>
          </Button>
        {/if}
        <Button id="session-import-cancel" variant="ghost" size="sm" disabled={sessions.importStep === 'importing'} onclick={() => sessions.closeImport()}>Cancel</Button>
        <Button size="sm" disabled={sessions.importStep === 'importing'} onclick={() => void sessions.importBundle()}>
          <LoadingActionContent busy={sessions.importStep === 'importing'} busyLabel="Importing…">
            <Upload size={13} aria-hidden="true" /><span>Import</span>
          </LoadingActionContent>
        </Button>
      </div>
    </Dialog>
  {/if}
</div>
