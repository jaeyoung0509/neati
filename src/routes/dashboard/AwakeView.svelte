<script lang="ts">
  import { onMount } from 'svelte';
  import { awakeStore } from '../../lib/stores/awake.svelte';
  import { settingsStore } from '../../lib/stores/settings.svelte';
  import { formatTimeUntil } from '../../lib/utils/format';
  import AwakeRuleEditor from '../../lib/components/AwakeRuleEditor.svelte';
  import AwakeRuleActions from '../../lib/components/AwakeRuleActions.svelte';
  import AwakeManualDurations from '../../lib/components/AwakeManualDurations.svelte';
  import LoadingActionContent from '../../lib/components/LoadingActionContent.svelte';
  import LoadingIndicator from '../../lib/components/LoadingIndicator.svelte';
  import { awaitingFirstAnswer } from '../../lib/utils/loadPhase';
  import { observeWhileVisible } from '../../lib/utils/visiblePolling';
  import {
    awakeAgentLabel,
    awakeRuleSummary,
    typedAwakeRule,
  } from '../../lib/utils/awake';
  import type { AwakeRule, AwakeRuleStatus } from '../../lib/models/types';
  import Button from '../../lib/components/Button.svelte';
  import Card from '../../lib/components/Card.svelte';
  import Badge from '../../lib/components/Badge.svelte';
  import PageHeader from '../../lib/components/PageHeader.svelte';
  import InlineNotice from '../../lib/components/InlineNotice.svelte';
  import {
    AlertCircle,
    AppWindow,
    Battery,
    Moon,
    Plus,
    Power,
    Shield,
    Zap,
  } from '@lucide/svelte';

  type EditorMode = 'basic' | 'advanced';

  let now = $state(Date.now());
  /** The open rule editor, or `null`. Each open gets a fresh draft. */
  let editor = $state<{ mode: EditorMode; rule: AwakeRule | null } | null>(null);

  onMount(() => {
    // UI countdown/read ownership follows the visible route. The backend
    // Keep Awake watcher remains independent and keeps enabled rules active.
    const stopCountdown = observeWhileVisible(() => { now = Date.now(); }, 1000);
    const stopRefresh = observeWhileVisible(() => { void awakeStore.refresh(); }, 5000);
    return () => {
      stopCountdown();
      stopRefresh();
    };
  });

  let awakeState = $derived(awakeStore.state);
  let rules = $derived(settingsStore.settings.awake_rules);
  let typedRules = $derived(rules.filter(typedAwakeRule));
  let legacyRules = $derived(rules.filter((rule) => !typedAwakeRule(rule)));
  let activeRule = $derived(rules.find((rule) => rule.id === awakeState.active_rule_id) ?? null);
  // Before the first state read answers, "Normal sleep is active" is not a result (#491).
  let checking = $derived(awaitingFirstAnswer(awakeStore.loadPhase));
  let unread = $derived(awakeStore.loadPhase === 'error');
  let pending = $derived(awakeStore.pending);

  function openBasicEditor(rule?: AwakeRule) {
    editor = { mode: 'basic', rule: rule ?? null };
  }

  function openAdvancedEditor(rule?: AwakeRule) {
    editor = { mode: 'advanced', rule: rule ?? null };
  }

  function getRuleEvaluation(ruleId: string) {
    return awakeState.rule_evaluations?.find((evaluation) => evaluation.rule_id === ruleId);
  }

  function formatCountdown(expiresAt: number) {
    const diffSeconds = Math.max(0, expiresAt - Math.floor(now / 1000));
    if (diffSeconds <= 0) return 'expiring now';
    const timeUntil = formatTimeUntil(expiresAt);
    return timeUntil ? `${timeUntil} remaining` : `${diffSeconds}s remaining`;
  }

  function statusLabel(status: AwakeRuleStatus): string {
    switch (status) {
      case 'active': return 'Ready';
      case 'waiting_application': return 'Waiting for app';
      case 'waiting_agent': return 'Waiting for an agent';
      case 'waiting_power': return 'Waiting for AC power';
      case 'invalid_application': return 'Application unavailable';
      case 'waiting_process': return 'Waiting for process';
      case 'disabled': return 'Disabled';
    }
  }

  function statusClass(status: AwakeRuleStatus): string {
    if (status === 'active') return 'status-chip-success text-success';
    if (status === 'waiting_power') return 'status-chip-warning text-warning';
    if (status === 'invalid_application') return 'status-chip-destructive text-destructive';
    if (status === 'disabled') return 'border-border/50 text-muted-foreground/70';
    return 'border-border bg-secondary/50 text-muted-foreground';
  }

</script>

<div class="space-y-6">
  <PageHeader
    title="Keep Awake"
    subtitle="Keep long-running work alive with explicit app, agent, power, and display choices."
    icon={Moon}
  >
    {#snippet badge()}
      {#if checking}
        <Badge variant="secondary">Checking</Badge>
      {:else if unread}
        <Badge variant="secondary">Unknown</Badge>
      {:else if awakeState.is_active}
        <Badge variant="warning" class="gap-1.5 font-medium">
          <span class="relative flex h-1.5 w-1.5" aria-hidden="true">
            <span class="animate-ping absolute inline-flex h-full w-full rounded-full bg-warning opacity-75"></span>
            <span class="relative inline-flex rounded-full h-1.5 w-1.5 bg-warning"></span>
          </span>
          Active
        </Badge>
      {:else}
        <Badge variant="secondary">Idle</Badge>
      {/if}
    {/snippet}

    {#snippet actions()}
      <div class="flex items-center gap-2 px-2.5 py-1.5 rounded-lg border border-border/70 bg-card/60 text-meta" aria-label="Current power source">
        {#if awakeState.power_source === 'ac'}
          <Zap size={14} class="text-success" aria-hidden="true" />
          <span class="font-medium text-foreground">Plugged In (AC)</span>
        {:else if awakeState.power_source === 'battery'}
          <Battery size={14} class="text-warning" aria-hidden="true" />
          <span class="font-medium text-foreground">Battery Power</span>
        {:else}
          <Shield size={14} class="text-muted-foreground" aria-hidden="true" />
          <span class="text-muted-foreground">Power: Unknown</span>
        {/if}
      </div>
    {/snippet}
  </PageHeader>

  {#if awakeState.last_error}
    <InlineNotice
      variant="destructive"
      title="Native Power Assertion Error"
      message={awakeState.last_error}
    />
  {/if}

  {#if awakeState.evaluation_health?.status === 'degraded'}
    <InlineNotice
      variant="warning"
      title="Keep Awake evaluation failed"
      message={`The background evaluation failed: ${awakeState.evaluation_health.last_failure_reason ?? 'unknown error'}. The displayed state is the last successful evaluation; the next interval retries automatically.`}
    />
  {:else if awakeState.evaluation_health?.last_failed_at != null}
    <InlineNotice
      variant="info"
      title="Keep Awake evaluation recovered"
      message={`The background evaluation recovered after: ${awakeState.evaluation_health.last_failure_reason ?? 'unknown error'}. The displayed state is current again.`}
    />
  {/if}

  {#if awakeStore.error}
    <InlineNotice
      variant="destructive"
      title="Keep Awake change not applied"
      message={awakeStore.error}
    />
  {/if}

  {#if settingsStore.error}
    <InlineNotice
      variant="destructive"
      title="Settings Save Error"
      message={settingsStore.error}
    />
  {/if}

  <Card class="p-5 {awakeState.is_active ? 'status-chip-warning' : 'bg-card'} transition-colors duration-normal">
    <div class="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
      <div class="space-y-1.5">
        <div class="flex items-center gap-2">
          {#if checking}
            <LoadingIndicator size="xs" class="text-primary" />
          {:else}
            <Power size={16} class={awakeState.is_active ? 'text-warning' : 'text-muted-foreground'} aria-hidden="true" />
          {/if}
          <h2 class="text-sm font-semibold text-foreground" role={checking ? 'status' : undefined}>
            {#if checking}
              Checking Keep Awake…
            {:else if unread}
              Keep Awake state unknown
            {:else if awakeState.is_active}
              {#if awakeState.manual_expires_at != null || awakeState.trigger_source?.includes('Manual')}
                Keep Awake Active (Manual Session)
              {:else if activeRule}
                Keep Awake Active ({activeRule.app_name})
              {:else}
                Keep Awake Active
              {/if}
            {:else}
              Normal sleep is active
            {/if}
          </h2>
        </div>

        <p class="text-meta text-muted-foreground leading-relaxed">
          {#if checking}
            Reading the current session and rule evaluation.
          {:else if unread}
            {awakeStore.readError ?? 'The current session could not be read.'} neati tries again while this page is open.
          {:else if awakeState.is_active}
            {#if awakeState.manual_expires_at != null}
              Manual session · <strong class="text-foreground font-mono">{formatCountdown(awakeState.manual_expires_at)}</strong> · {awakeState.behavior === 'keep_display_awake' ? 'Display stays awake.' : 'Display may sleep.'}
            {:else if awakeState.trigger_source?.includes('Manual')}
              Manual indefinite session active · {awakeState.behavior === 'keep_display_awake' ? 'Display stays awake.' : 'Display may sleep.'}
            {:else if activeRule}
              {awakeRuleSummary(activeRule)}
            {:else}
              {awakeState.trigger_source || 'System sleep is currently prevented by neati.'}
            {/if}
          {:else}
            Watching {awakeState.active_rules_count} enabled {awakeState.active_rules_count === 1 ? 'automatic rule' : 'automatic rules'}. Manual Keep Awake always takes priority.
          {/if}
        </p>
      </div>

      {#if !checking && !unread && awakeState.is_active}
        {#if awakeState.manual_expires_at != null || awakeState.trigger_source?.includes('Manual')}
          <Button variant="destructive" size="sm" disabled={pending !== null} aria-busy={pending?.kind === 'stop-manual'} onclick={() => void awakeStore.disableManual()}>
            <LoadingActionContent busy={pending?.kind === 'stop-manual'} busyLabel="Stopping…">Stop Manual Session</LoadingActionContent>
          </Button>
        {:else if activeRule}
          {@const disabling = pending?.kind === 'rule' && pending.ruleId === activeRule.id}
          <Button variant="outline" size="sm" class="text-meta border-warning/40 hover:bg-warning/20" disabled={pending !== null || !settingsStore.hasLoaded} aria-busy={disabling} onclick={() => void awakeStore.toggleRule(activeRule.id)}>
            <LoadingActionContent busy={disabling} busyLabel="Disabling…">Disable {activeRule.app_name} Rule</LoadingActionContent>
          </Button>
        {/if}
      {/if}
    </div>
  </Card>

  <AwakeManualDurations />

  {#if rules.length > 0 && rules.every((rule) => !rule.enabled)}
    <Card class="p-4 status-panel-ai space-y-3">
      <div class="flex items-start gap-3">
        <div class="h-8 w-8 rounded-lg bg-ai/10 text-ai flex items-center justify-center shrink-0 mt-0.5">
          <AlertCircle size={16} aria-hidden="true" />
        </div>
        <div class="space-y-1 flex-1">
          <div class="text-meta font-semibold text-foreground">Automatic rules are off</div>
          <p class="text-meta text-muted-foreground leading-relaxed">
            Rules are saved disabled to protect battery life. Review the rule summary, then enable only the rules you explicitly want to run.
          </p>
          <div class="pt-2">
            <Button variant="primary" size="sm" disabled={pending !== null || !settingsStore.hasLoaded} aria-busy={pending?.kind === 'recommended-rules'} onclick={() => void awakeStore.enableRecommendedRules()}>
              <LoadingActionContent busy={pending?.kind === 'recommended-rules'} busyLabel="Enabling…">Enable recommended AC-only rules</LoadingActionContent>
            </Button>
          </div>
        </div>
      </div>
    </Card>
  {/if}

  <section class="space-y-3 pt-2" aria-labelledby="automatic-heading">
    <div class="flex flex-col gap-3 sm:flex-row sm:items-end sm:justify-between">
      <div>
        <h2 id="automatic-heading" class="text-sm font-semibold tracking-tight text-foreground">Automatic app rules</h2>
        <p class="text-meta text-muted-foreground mt-0.5">Build an app-and-agent condition in plain language. Selected agents use OR semantics.</p>
      </div>
      <Button variant="outline" size="sm" onclick={() => openBasicEditor()} class="gap-1.5">
        <Plus size={13} aria-hidden="true" />
        Add app rule
      </Button>
    </div>

    {#if typedRules.length === 0}
      <Card class="p-7 text-center space-y-3 bg-card/40 border-dashed">
        <div class="h-10 w-10 rounded-full bg-ai/10 text-ai flex items-center justify-center mx-auto">
          <AppWindow size={20} aria-hidden="true" />
        </div>
        <div class="space-y-1">
          <h3 class="text-sm font-semibold text-foreground">Choose an installed app to get started</h3>
          <p class="text-meta text-muted-foreground max-w-lg mx-auto leading-relaxed">
            For example, choose Warp, select Codex or another agent, and keep the computer awake only while plugged in. Leaving agents empty creates an app-only rule.
          </p>
        </div>
        <Button variant="primary" size="sm" onclick={() => openBasicEditor()} class="gap-1.5">
          <Plus size={13} aria-hidden="true" />
          Build an app rule
        </Button>
      </Card>
    {:else}
      <div class="space-y-2">
        {#each typedRules as rule (rule.id)}
          {@const evaluation = getRuleEvaluation(rule.id)}
          <Card class="p-3.5 bg-card/70 border-border/80">
            <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
              <div class="min-w-0 flex-1 space-y-2">
                <div class="flex flex-wrap items-center gap-2">
                  <span class="font-medium text-foreground truncate">{rule.application?.display_name ?? rule.app_name}</span>
                  {#if evaluation}
                    <span class="px-2 py-0.5 rounded-full text-caption font-medium border {statusClass(evaluation.status)}" title={evaluation.status === 'waiting_agent' ? 'The selected app is running, but none of the selected agents is running.' : undefined}>
                      {statusLabel(evaluation.status)}
                    </span>
                  {/if}
                  {#if !rule.enabled}
                    <Badge variant="secondary">Off</Badge>
                  {/if}
                </div>
                <p class="text-meta text-muted-foreground leading-relaxed">{awakeRuleSummary(rule)}</p>
                <div class="flex flex-wrap items-center gap-1.5 text-caption text-muted-foreground">
                  {#if (rule.agent_ids ?? []).length > 0}
                    <span>Any selected agent:</span>
                    {#each rule.agent_ids ?? [] as agentId}
                      <span class="inline-flex items-center rounded-full border status-chip-ai px-2 py-0.5 text-ai">{awakeAgentLabel(agentId)}</span>
                    {/each}
                  {:else}
                    <span>App alone triggers this rule</span>
                  {/if}
                  <span class="mx-1" aria-hidden="true">·</span>
                  <span>{rule.power_condition === 'ac_power_only' ? 'Plugged in only' : 'AC or battery'}</span>
                  <span class="mx-1" aria-hidden="true">·</span>
                  <span>{rule.behavior === 'keep_display_awake' ? 'Display stays awake' : 'Display may sleep'}</span>
                </div>
                {#if rule.application?.path}
                  <p class="truncate text-caption text-muted-foreground/80" title={rule.application.path}>Selected path: {rule.application.path}</p>
                {/if}
              </div>

              <AwakeRuleActions {rule} label={`${rule.app_name} rule`} onEdit={() => openBasicEditor(rule)} />
            </div>
          </Card>
        {/each}
      </div>
    {/if}
  </section>

  <details class="group rounded-xl border border-border/70 bg-card/30">
    <summary class="flex cursor-pointer list-none items-center justify-between gap-3 p-4 text-meta font-semibold text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
      <span>Advanced / legacy rules ({legacyRules.length})</span>
      <span class="text-meta text-muted-foreground group-open:rotate-180 transition-transform" aria-hidden="true">⌄</span>
    </summary>
    <div class="border-t border-border/60 p-4 space-y-3">
      <p class="text-meta text-muted-foreground">
        Older custom rules are preserved losslessly. Their raw process fragments are evaluated only here and never mixed into the basic app builder.
      </p>
      <Button variant="outline" size="sm" onclick={() => openAdvancedEditor()} class="gap-1.5">
        <Plus size={13} aria-hidden="true" />
        Add custom legacy rule
      </Button>
      {#if legacyRules.length === 0}
        <p class="text-meta text-muted-foreground">No legacy rules are configured.</p>
      {:else}
        <div class="space-y-2">
          {#each legacyRules as rule (rule.id)}
            {@const evaluation = getRuleEvaluation(rule.id)}
            <div class="flex flex-col gap-3 rounded-lg border border-border/70 bg-card/60 p-3 sm:flex-row sm:items-start sm:justify-between">
              <div class="min-w-0 space-y-1.5">
                <div class="flex flex-wrap items-center gap-2">
                  <span class="text-meta font-medium text-foreground">{rule.app_name}</span>
                  {#if evaluation}
                    <span class="px-2 py-0.5 rounded-full text-caption font-medium border {statusClass(evaluation.status)}">{statusLabel(evaluation.status)}</span>
                  {/if}
                  {#if !rule.enabled}<Badge variant="secondary">Off</Badge>{/if}
                </div>
                <p class="font-mono text-caption text-muted-foreground break-all">Primary: {rule.executable_pattern}</p>
                {#if rule.requires_process_pattern}
                  <p class="font-mono text-caption text-muted-foreground break-all">Also require: {rule.requires_process_pattern}</p>
                {/if}
              </div>
              <AwakeRuleActions {rule} label={`${rule.app_name} legacy rule`} onEdit={() => openAdvancedEditor(rule)} />
            </div>
          {/each}
        </div>
      {/if}
    </div>
  </details>

  {#if editor}
    {#key editor}
      <AwakeRuleEditor mode={editor.mode} rule={editor.rule} onClose={() => (editor = null)} />
    {/key}
  {/if}
</div>
