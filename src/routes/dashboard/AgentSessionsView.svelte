<script lang="ts">
  import { onMount } from 'svelte';
  import { Archive, FolderX, MessagesSquare, RotateCw } from '@lucide/svelte';
  import { agentSessionsStore as sessions } from '../../lib/stores/agentSessions.svelte';
  import { awaitingFirstAnswer } from '../../lib/utils/loadPhase';
  import { formatTimeAgo } from '../../lib/utils/format';
  import {
    SESSION_TOOL_LABELS,
    folderStateLabel,
    sessionIssueLabel,
    sessionLabel,
  } from '../../lib/utils/agentSessions';
  import Badge from '../../lib/components/Badge.svelte';
  import Button from '../../lib/components/Button.svelte';
  import ByteValue from '../../lib/components/ByteValue.svelte';
  import EmptyState from '../../lib/components/EmptyState.svelte';
  import InlineNotice from '../../lib/components/InlineNotice.svelte';
  import LoadingActionContent from '../../lib/components/LoadingActionContent.svelte';
  import LoadingState from '../../lib/components/LoadingState.svelte';
  import PageHeader from '../../lib/components/PageHeader.svelte';

  /**
   * Agent sessions (#508): Claude Code and Codex sessions by project,
   * read-only. neati never changes or removes a session file here.
   */
  onMount(() => {
    void sessions.load();
  });

  let inventory = $derived(sessions.inventory);
  let projects = $derived(inventory?.projects ?? []);
  let reading = $derived(sessions.reading);
  let anyTool = $derived(inventory?.tools.some((tool) => tool.present) ?? false);
</script>

<div class="space-y-6">
  <PageHeader
    title="Sessions"
    subtitle="Claude Code and Codex sessions on this Mac, grouped by project. neati reads them and never changes or removes them."
    icon={MessagesSquare}
  >
    {#snippet actions()}
      <Button
        variant="outline"
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
      {#each projects as project (project.key)}
        {@const folderLabel = folderStateLabel(project.folder)}
        <section class="rounded-xl border border-border bg-card" aria-label={project.display_name}>
          <header class="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-border px-4 py-3">
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
</div>
