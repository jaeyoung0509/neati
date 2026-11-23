<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { Check, Copy, Terminal } from '@lucide/svelte';
  import type { AgentSkillTarget, CommandLineStatus } from '../../models/types';
  import { describeError } from '../../utils/errors';
  import { tauriGetCommandLineStatus, tauriInstallAgentSkill } from '../../utils/tauri';
  import { AGENT_SKILL_LABELS, agentSkillPresentation, commandLineSummary } from '../../utils/commandLine';
  import Badge from '../Badge.svelte';
  import Button from '../Button.svelte';
  import Card from '../Card.svelte';
  import LoadingActionContent from '../LoadingActionContent.svelte';
  import LoadingState from '../LoadingState.svelte';

  /**
   * The `neati` command and the agent skill (#500). The window reports where
   * `neati` resolves and offers a link command to copy; it never edits a PATH
   * folder. Installing the skill writes neati's own file and nothing else.
   */
  let { initialStatus = null }: { initialStatus?: CommandLineStatus | null } = $props();

  // A given status seeds the section once (tests, previews); it is not a
  // binding, so later loads replace it.
  const seeded = untrack(() => initialStatus);
  let status = $state<CommandLineStatus | null>(seeded);
  let phase = $state<'loading' | 'ready' | 'error'>(seeded ? 'ready' : 'loading');
  let error = $state<string | null>(null);
  let installing = $state<AgentSkillTarget | null>(null);
  let installMessage = $state<string | null>(null);
  let copied = $state(false);

  onMount(() => {
    if (!seeded) void load();
  });

  async function load() {
    phase = 'loading';
    error = null;
    try {
      status = await tauriGetCommandLineStatus();
      phase = 'ready';
    } catch (cause) {
      error = describeError(cause);
      phase = 'error';
    }
  }

  async function install(target: AgentSkillTarget) {
    if (installing) return;
    installing = target;
    installMessage = null;
    try {
      const result = await tauriInstallAgentSkill(target);
      installMessage = `${result.replaced ? 'Updated' : 'Installed'} for ${AGENT_SKILL_LABELS[target]}. Start a new session to load it.`;
      status = await tauriGetCommandLineStatus();
    } catch (cause) {
      installMessage = describeError(cause);
    } finally {
      installing = null;
    }
  }

  async function copyLink() {
    if (!status?.link_command || typeof navigator === 'undefined' || !navigator.clipboard) return;
    await navigator.clipboard.writeText(status.link_command);
    copied = true;
    setTimeout(() => (copied = false), 2000);
  }
</script>

<div class="space-y-3" data-settings-command-line>
  <div>
    <h3 class="text-meta font-semibold text-muted-foreground tracking-normal">Command line and agents</h3>
    <p class="mt-1 text-meta text-muted-foreground">
      <code class="font-mono">neati clean</code> previews what Clean removes; nothing is removed without <code class="font-mono">--yes</code>.
    </p>
  </div>
  <Card class="p-4 space-y-4 bg-card">
    {#if phase === 'loading'}
      <LoadingState message="Checking the neati command…" class="py-6" />
    {:else if phase === 'error' || !status}
      <div class="flex items-center justify-between gap-3 text-meta">
        <p class="text-destructive" role="alert">{error ?? 'The command line status is unavailable.'}</p>
        <Button size="sm" variant="outline" onclick={() => void load()}>Retry</Button>
      </div>
    {:else}
      <div class="space-y-2 text-meta">
        <div class="flex items-center gap-2 font-medium text-foreground">
          <Terminal size={14} aria-hidden="true" />
          <span>neati command</span>
        </div>
        <p class="text-muted-foreground break-all">{commandLineSummary(status)}</p>
        {#if !status.linked_here && status.link_command}
          <div class="flex items-center gap-2">
            <code class="min-w-0 flex-1 truncate rounded-md bg-secondary px-2 py-1 font-mono text-caption text-foreground" title={status.link_command}>{status.link_command}</code>
            <Button size="sm" variant="outline" onclick={() => void copyLink()} aria-label="Copy the link command" title="Copy the link command">
              {#if copied}<Check size={14} aria-hidden="true" />{:else}<Copy size={14} aria-hidden="true" />{/if}
            </Button>
          </div>
          <p class="text-caption text-muted-foreground">Run it in Terminal, or install with Homebrew, which links it for you.</p>
        {/if}
      </div>

      <div class="space-y-2 text-meta">
        <div class="font-medium text-foreground">Agent skill</div>
        <p class="text-muted-foreground">Teaches Claude Code or Codex to preview first and ask before anything is removed.</p>
        <ul class="space-y-1.5">
          {#each status.skills as skill (skill.target)}
            {@const view = agentSkillPresentation(skill.state)}
            <li class="flex items-center justify-between gap-3">
              <div class="min-w-0">
                <span class="font-medium text-foreground">{AGENT_SKILL_LABELS[skill.target]}</span>
                <Badge variant={view.tone} class="ml-2">{view.status}</Badge>
                <p class="truncate font-mono text-caption text-muted-foreground" title={skill.path}>{skill.path}</p>
              </div>
              {#if view.action}
                <Button size="sm" variant="outline" disabled={installing !== null} onclick={() => void install(skill.target)}>
                  <LoadingActionContent busy={installing === skill.target} busyLabel="Installing…">{view.action}</LoadingActionContent>
                </Button>
              {/if}
            </li>
          {/each}
        </ul>
        {#if installMessage}
          <p class="text-caption text-muted-foreground" role="status">{installMessage}</p>
        {/if}
      </div>
    {/if}
  </Card>
</div>
