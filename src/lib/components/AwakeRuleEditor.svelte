<script lang="ts">
  import { untrack } from 'svelte';
  import { awakeStore } from '../stores/awake.svelte';
  import { settingsStore } from '../stores/settings.svelte';
  import { tauriPickKeepAwakeApplication } from '../utils/tauri';
  import {
    AWAKE_AGENT_OPTIONS,
    awakeAgentLabel,
    awakeRuleSummary,
    selectedAwakeAgents,
    toggleAwakeAgent,
  } from '../utils/awake';
  import type {
    ApplicationIdentity,
    AwakeAgentId,
    AwakeBehavior,
    AwakeRule,
    PowerCondition,
  } from '../models/types';
  import Button from './Button.svelte';
  import LoadingActionContent from './LoadingActionContent.svelte';
  import Dialog from './Dialog.svelte';
  import { AppWindow, Check, FolderOpen, X } from '@lucide/svelte';

  /**
   * Builds or edits one Keep Awake rule. `basic` rules bind a picked
   * application (and optional agents); `advanced` keeps a legacy raw rule raw.
   * The editor owns its draft; the caller only decides when it is open.
   */
  interface Props {
    mode: 'basic' | 'advanced';
    /** The rule being edited, or `null` for a new rule. */
    rule: AwakeRule | null;
    onClose: () => void;
  }

  let { mode: editorMode, rule: editingRule, onClose }: Props = $props();

  // The draft is seeded once from the rule the editor was opened with.
  const initial = untrack(() => editingRule);
  const basic = untrack(() => editorMode === 'basic');
  let editingRuleId = initial?.id ?? null;
  let newApplication = $state<ApplicationIdentity | null>(basic ? initial?.application ?? null : null);
  let selectedAgents = $state<AwakeAgentId[]>(basic && initial ? selectedAwakeAgents(initial) : []);
  let newAppName = $state(initial?.app_name ?? '');
  let newExecutable = $state(initial?.executable_pattern ?? '');
  let newRequiresPattern = $state(initial?.requires_process_pattern ?? '');
  let newBehavior = $state<AwakeBehavior>(initial?.behavior ?? 'prevent_system_sleep');
  let newPowerCondition = $state<PowerCondition>(initial?.power_condition ?? 'ac_power_only');
  let isPickingApp = $state(false);
  let isSavingRule = $state(false);
  let pickerError = $state<string | null>(null);

  function closeRuleEditor() {
    onClose();
  }

  async function pickApplication() {
    if (isPickingApp) return;
    isPickingApp = true;
    pickerError = null;
    try {
      const selection = await tauriPickKeepAwakeApplication();
      if (!selection) return;

      if (editorMode === 'basic') {
        newApplication = {
          display_name: selection.name,
          executable_name: selection.executable_pattern,
          path: selection.path,
        };
      } else {
        // Advanced rules remain explicitly raw. The picker only helps fill the
        // primary legacy fragment and never silently upgrades the rule shape.
        newAppName = selection.name;
        newExecutable = selection.executable_pattern;
      }
    } catch (error: unknown) {
      pickerError = error instanceof Error ? error.message : String(error);
    } finally {
      isPickingApp = false;
    }
  }

  function removeAgent(agentId: AwakeAgentId) {
    selectedAgents = selectedAgents.filter((candidate) => candidate !== agentId);
  }

  function isEditorValid(): boolean {
    if (editorMode === 'basic') return newApplication !== null;
    return Boolean(newAppName.trim() && newExecutable.trim());
  }

  async function saveRule() {
    if (isSavingRule || awakeStore.isLoading || !isEditorValid()) return;
    isSavingRule = true;
    pickerError = null;
    const existing = editingRule;
    const id = editingRuleId ?? `rule.${Date.now()}`;
    const enabled = existing?.enabled ?? false;
    const rule: AwakeRule = editorMode === 'basic'
      ? {
          id,
          app_name: newApplication?.display_name.trim() ?? '',
          executable_pattern: newApplication?.executable_name.trim() ?? '',
          requires_process_pattern: null,
          application: newApplication,
          agent_ids: [...selectedAgents],
          behavior: newBehavior,
          power_condition: newPowerCondition,
          enabled,
        }
      : {
          id,
          app_name: newAppName.trim(),
          executable_pattern: newExecutable.trim(),
          requires_process_pattern: newRequiresPattern.trim() || null,
          application: null,
          agent_ids: [],
          behavior: newBehavior,
          power_condition: newPowerCondition,
          enabled,
        };

    try {
      const saved = editingRuleId
        ? await awakeStore.updateRule(rule)
        : await awakeStore.addRule(rule);
      if (!saved) {
        pickerError = settingsStore.error ?? 'Could not save the rule. Please try again.';
        return;
      }
      closeRuleEditor();
    } catch (error: unknown) {
      pickerError = error instanceof Error ? error.message : String(error);
    } finally {
      isSavingRule = false;
    }
  }

</script>

<Dialog
  onCancel={() => { if (!isSavingRule) closeRuleEditor(); }} initialFocus="#rule-editor-title"
  aria-labelledby="rule-editor-title"
  size="xl"
  >
    <div class="flex items-start justify-between gap-3 border-b border-border/60 pb-3">
      <div>
        <h2 id="rule-editor-title" tabindex="-1" class="text-sm font-semibold">{editorMode === 'basic' ? 'Build an app rule' : 'Edit legacy process rule'}</h2>
        <p class="mt-1 text-meta text-muted-foreground">{editorMode === 'basic' ? 'Choose what must be present before neati holds a power assertion.' : 'Use raw process fragments only for custom or older rules.'}</p>
      </div>
      <button type="button" onclick={closeRuleEditor} disabled={isSavingRule} class="h-8 w-8 rounded-lg text-muted-foreground hover:bg-secondary hover:text-foreground disabled:opacity-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring" title="Close rule editor" aria-label="Close rule editor">
        <X size={16} class="mx-auto" aria-hidden="true" />
      </button>
    </div>

    {#if editorMode === 'basic'}
      <div class="space-y-5 pt-5">
        <fieldset class="space-y-2">
          <legend class="text-meta font-semibold text-foreground">When this application is open</legend>
          <p class="text-meta text-muted-foreground">Choose a native application. The saved identity includes its exact bundle or executable path.</p>
          <button type="button" onclick={() => void pickApplication()} disabled={isPickingApp} class="flex w-full items-center gap-3 rounded-xl border status-panel-ai p-3 text-left transition-colors hover:bg-ai/10 disabled:opacity-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
            <div class="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-ai/10 text-ai"><AppWindow size={18} aria-hidden="true" /></div>
            <div class="min-w-0 flex-1">
              <p class="text-meta font-medium text-foreground">{isPickingApp ? 'Opening Applications…' : newApplication ? 'Change selected application' : 'Choose from installed applications'}</p>
              <p class="mt-0.5 truncate text-caption text-muted-foreground">{newApplication ? `${newApplication.display_name} · ${newApplication.executable_name}` : 'Cancellation leaves the rule unchanged.'}</p>
            </div>
            <FolderOpen size={15} class="shrink-0 text-muted-foreground" aria-hidden="true" />
          </button>
          {#if newApplication}
            <div class="rounded-lg border border-border/70 bg-secondary/30 px-3 py-2 text-caption text-muted-foreground">
              <div class="flex items-center justify-between gap-3"><span class="font-medium text-foreground">{newApplication.display_name}</span><span class="text-success">Verified picker identity</span></div>
              <p class="mt-1 truncate" title={newApplication.path}>{newApplication.path}</p>
            </div>
          {/if}
        </fieldset>

        <fieldset class="space-y-2">
          <legend class="text-meta font-semibold text-foreground">And any of these agents is active <span class="font-normal text-muted-foreground">(optional)</span></legend>
          <p class="text-meta text-muted-foreground">Select one or more. The app must be open and at least one selected agent must be active. Leave empty for an app-only rule.</p>
          <div class="grid grid-cols-1 gap-2 sm:grid-cols-2" role="group" aria-label="Known Keep Awake agents">
            {#each AWAKE_AGENT_OPTIONS as agent}
              {@const selected = selectedAgents.includes(agent.id)}
              <button type="button" aria-pressed={selected} onclick={() => (selectedAgents = toggleAwakeAgent(selectedAgents, agent.id))} class="flex items-center gap-2 rounded-lg border px-3 py-2 text-left transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring {selected ? 'status-chip-ai text-foreground' : 'border-border/70 text-muted-foreground hover:bg-secondary/50'}">
                <span class="flex h-4 w-4 items-center justify-center rounded border {selected ? 'border-ai bg-ai text-white' : 'border-border'}" aria-hidden="true">{#if selected}<Check size={11} />{/if}</span>
                <span class="min-w-0"><span class="block text-meta font-medium">{agent.label}</span><span class="block text-caption text-muted-foreground">{agent.hint}</span></span>
              </button>
            {/each}
          </div>
          {#if selectedAgents.length > 0}
            <div class="flex flex-wrap items-center gap-1.5 pt-1" aria-label="Selected agents">
              <span class="text-caption text-muted-foreground">Any of:</span>
              {#each selectedAgents as agentId}
                <span class="inline-flex items-center gap-1 rounded-full border status-chip-ai pl-2 pr-1 py-0.5 text-caption text-ai">
                  {awakeAgentLabel(agentId)}
                  <button type="button" onclick={() => removeAgent(agentId)} class="rounded-full p-0.5 hover:bg-ai/20 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring" title={`Remove ${awakeAgentLabel(agentId)}`} aria-label={`Remove ${awakeAgentLabel(agentId)}`}><X size={11} aria-hidden="true" /></button>
                </span>
              {/each}
            </div>
          {/if}
        </fieldset>

        <fieldset class="space-y-2">
          <legend class="text-meta font-semibold text-foreground">Power</legend>
          <div class="grid grid-cols-1 gap-2 sm:grid-cols-2">
            <label class="flex cursor-pointer gap-2 rounded-lg border p-3 transition-colors {newPowerCondition === 'ac_power_only' ? 'status-panel-success' : 'border-border/70 hover:bg-secondary/40'}">
              <input type="radio" name="rule-power" value="ac_power_only" bind:group={newPowerCondition} class="mt-0.5 accent-success" />
              <span><span class="block text-meta font-medium">Plugged in only</span><span class="mt-0.5 block text-caption text-muted-foreground">Recommended to avoid battery drain. Unknown power is ineligible.</span></span>
            </label>
            <label class="flex cursor-pointer gap-2 rounded-lg border p-3 transition-colors {newPowerCondition === 'always' ? 'status-panel-warning' : 'border-border/70 hover:bg-secondary/40'}">
              <input type="radio" name="rule-power" value="always" bind:group={newPowerCondition} class="mt-0.5 accent-warning" />
              <span><span class="block text-meta font-medium">AC or battery</span><span class="mt-0.5 block text-caption text-muted-foreground">Use only when keeping work alive on battery is intentional.</span></span>
            </label>
          </div>
        </fieldset>

        <fieldset class="space-y-2">
          <legend class="text-meta font-semibold text-foreground">Sleep behavior</legend>
          <div class="grid grid-cols-1 gap-2 sm:grid-cols-2">
            <label class="flex cursor-pointer gap-2 rounded-lg border p-3 transition-colors {newBehavior === 'prevent_system_sleep' ? 'border-primary/40 bg-primary/5' : 'border-border/70 hover:bg-secondary/40'}">
              <input type="radio" name="rule-behavior" value="prevent_system_sleep" bind:group={newBehavior} class="mt-0.5 accent-primary" />
              <span><span class="block text-meta font-medium">Computer awake</span><span class="mt-0.5 block text-caption text-muted-foreground">Work continues; the display may sleep.</span></span>
            </label>
            <label class="flex cursor-pointer gap-2 rounded-lg border p-3 transition-colors {newBehavior === 'keep_display_awake' ? 'border-primary/40 bg-primary/5' : 'border-border/70 hover:bg-secondary/40'}">
              <input type="radio" name="rule-behavior" value="keep_display_awake" bind:group={newBehavior} class="mt-0.5 accent-primary" />
              <span><span class="block text-meta font-medium">Computer + display awake</span><span class="mt-0.5 block text-caption text-muted-foreground">Use when the visible display must remain on.</span></span>
            </label>
          </div>
        </fieldset>

        {#if newApplication}
          <div class="rounded-lg border border-border/70 bg-secondary/25 p-3" aria-live="polite">
            <p class="text-caption font-semibold tracking-normal text-muted-foreground">Before saving</p>
            <p class="mt-1 text-meta leading-relaxed text-foreground">{awakeRuleSummary({ id: 'preview', app_name: newApplication.display_name, executable_pattern: newApplication.executable_name, requires_process_pattern: null, application: newApplication, agent_ids: selectedAgents, behavior: newBehavior, power_condition: newPowerCondition, enabled: false })}</p>
            <p class="mt-1 text-caption text-warning">New rules are saved off. Turn the rule on after reviewing it.</p>
          </div>
        {/if}
      </div>
    {:else}
      <div class="space-y-4 pt-5">
        <div class="rounded-lg border status-panel-warning p-3 text-meta text-muted-foreground">Legacy matching may inspect process name, executable path, and command line. Keep this path for older/custom workflows; typed app rules are safer for known agents.</div>
        <div class="space-y-1.5"><label for="legacy-app-name" class="text-meta font-medium text-muted-foreground">Rule name</label><input id="legacy-app-name" type="text" bind:value={newAppName} placeholder="e.g. Blender render" class="h-9 w-full rounded-lg border border-border bg-background px-3 text-meta text-foreground focus:outline-none focus:ring-1 focus:ring-ring" /></div>
        <div class="space-y-1.5"><label for="legacy-primary" class="text-meta font-medium text-muted-foreground">Executable fragments</label><input id="legacy-primary" type="text" bind:value={newExecutable} placeholder="custom process fragment" class="h-9 w-full rounded-lg border border-border bg-background px-3 font-mono text-meta text-foreground focus:outline-none focus:ring-1 focus:ring-ring" /></div>
        <div class="space-y-1.5"><label for="legacy-required" class="text-meta font-medium text-muted-foreground">Also require process (optional)</label><input id="legacy-required" type="text" bind:value={newRequiresPattern} placeholder="secondary fragment or pipe-separated OR values" class="h-9 w-full rounded-lg border border-border bg-background px-3 font-mono text-meta text-foreground focus:outline-none focus:ring-1 focus:ring-ring" /></div>
        <div class="grid grid-cols-1 gap-2 sm:grid-cols-2">
          <label class="flex cursor-pointer gap-2 rounded-lg border border-border/70 p-3"><input type="radio" name="legacy-power" value="ac_power_only" bind:group={newPowerCondition} class="mt-0.5 accent-success" /><span class="text-meta">Plugged in only</span></label>
          <label class="flex cursor-pointer gap-2 rounded-lg border border-border/70 p-3"><input type="radio" name="legacy-power" value="always" bind:group={newPowerCondition} class="mt-0.5 accent-warning" /><span class="text-meta">AC or battery</span></label>
        </div>
        <div class="grid grid-cols-1 gap-2 sm:grid-cols-2">
          <label class="flex cursor-pointer gap-2 rounded-lg border border-border/70 p-3"><input type="radio" name="legacy-behavior" value="prevent_system_sleep" bind:group={newBehavior} class="mt-0.5 accent-primary" /><span class="text-meta">Computer awake; display may sleep</span></label>
          <label class="flex cursor-pointer gap-2 rounded-lg border border-border/70 p-3"><input type="radio" name="legacy-behavior" value="keep_display_awake" bind:group={newBehavior} class="mt-0.5 accent-primary" /><span class="text-meta">Computer + display awake</span></label>
        </div>
      </div>
    {/if}

    {#if pickerError}
      <p class="mt-4 rounded-lg border status-panel-destructive px-3 py-2 text-meta text-destructive" role="alert">{pickerError}</p>
    {/if}

    <div class="flex justify-end gap-2 border-t border-border/60 pt-4 mt-5">
      <Button variant="ghost" size="sm" onclick={closeRuleEditor} disabled={isSavingRule}>Cancel</Button>
      <!-- Another Keep Awake change must finish before this one is saved (#491). -->
      <Button variant="primary" size="sm" onclick={() => void saveRule()} disabled={!isEditorValid() || isSavingRule || awakeStore.isLoading} aria-busy={isSavingRule}>
        <LoadingActionContent busy={isSavingRule} busyLabel="Saving…">{editingRuleId ? 'Save changes' : 'Save rule'}</LoadingActionContent>
      </Button>
    </div>
</Dialog>
