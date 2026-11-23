<script lang="ts">
  import { onMount, tick, type Snippet } from 'svelte';
  import { Trash2 } from '@lucide/svelte';
  import type { RemovalPlanPreview } from '../models/types';
  import { formatCountdown, ttlRemaining } from '../utils/format';
  import { observeWhileVisible } from '../utils/visiblePolling';
  import { countLabel, PLAN_EXPIRING_SOON_SECONDS } from '../utils/removalPlan';
  import Button from './Button.svelte';
  import Card from './Card.svelte';
  import ElapsedTime from './ElapsedTime.svelte';
  import LoadingActionContent from './LoadingActionContent.svelte';

  /**
   * The one-shot permanent-removal review shared by Projects, Large Files and
   * Apps (#416, #491). The view supplies its own copy and the backend plan;
   * this card owns the countdown, expiry recovery focus and the busy and
   * disabled states of Cancel and the destructive confirm.
   */
  interface Props {
    plan: RemovalPlanPreview;
    title: string;
    /** The view's item count and size line. */
    summary: string;
    confirmLabel: string;
    /** What happens on confirm, shown while the plan is valid. */
    note: string;
    /** Prefix the note with the plan's expiry time and countdown. */
    showExpiryTime?: boolean;
    expiredMessage: string;
    expiryActionId: string;
    expiryActionLabel: string;
    onExpiryAction: () => void;
    /** Offers Dismiss next to the expiry action. */
    onDismiss?: () => void;
    /** Tooltip on the confirm button once the plan has expired. */
    expiredTitle?: string;
    executing: boolean;
    /** A view-owned consent the confirm still waits for. */
    confirmBlocked?: boolean;
    /** Stacked layout for a narrow detail pane. */
    compact?: boolean;
    onConfirm: () => void;
    onCancel: () => void;
    /** Exact scope, between the header and the plan notes. */
    children?: Snippet;
    /** Consent shown while the plan is valid, after the note. */
    footer?: Snippet;
  }

  let {
    plan, title, summary, confirmLabel, note, showExpiryTime = false,
    expiredMessage, expiryActionId, expiryActionLabel, onExpiryAction, onDismiss, expiredTitle = '',
    executing, confirmBlocked = false, compact = false, onConfirm, onCancel, children, footer,
  }: Props = $props();

  let now = $state(Date.now());
  onMount(() => observeWhileVisible(() => { now = Date.now(); }, 1000));

  let remainingSecs = $derived(ttlRemaining(plan.expires_at, now));
  let expired = $derived(remainingSecs === 0);
  let expiringSoon = $derived(remainingSecs > 0 && remainingSecs <= PLAN_EXPIRING_SOON_SECONDS);
  let expiresAt = $derived(new Date(plan.expires_at * 1000).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }));
  let surface = $derived(expired ? 'status-panel-destructive' : expiringSoon ? 'status-chip-warning' : 'status-panel-warning');
  let chip = $derived(expired ? 'status-chip-destructive text-destructive' : expiringSoon ? 'status-chip-warning text-warning' : 'border-border bg-secondary text-muted-foreground');

  // Expiry moves focus to the recovery action once per plan, so keyboard users land on it.
  let focusedPlanId: string | null = null;
  $effect(() => {
    if (!expired || focusedPlanId === plan.id) return;
    focusedPlanId = plan.id;
    void tick().then(() => document.getElementById(expiryActionId)?.focus());
  });

  // Removal reports no steps, so the card shows how long it has been running.
  let executingSince = $state<number | null>(null);
  $effect(() => { executingSince = executing ? Date.now() : null; });
</script>

<Card class="{compact ? 'p-4' : 'p-5'} space-y-3 {surface}">
  <div class="flex flex-col gap-3 {compact ? '' : '@2xl:flex-row @2xl:items-center @2xl:justify-between'}" data-removal-plan>
    <div class="min-w-0">
      <div class="flex flex-wrap items-center gap-2 text-sm font-semibold">
        {title}
        <span class="rounded border px-1.5 py-0.5 font-mono text-caption {chip}">{formatCountdown(remainingSecs)}</span>
      </div>
      <p class="mt-1 text-meta text-muted-foreground">{summary}</p>
    </div>
    <!-- Cancel stays beside the confirm; the copy wraps instead. -->
    <div class="flex shrink-0 flex-wrap items-center gap-2">
      <Button variant="ghost" size="sm" disabled={executing} onclick={onCancel}>Cancel</Button>
      <Button variant="destructive" size="md" onclick={onConfirm} disabled={executing || expired || confirmBlocked} aria-busy={executing} class="gap-1.5" title={expired ? expiredTitle : ''}>
        <LoadingActionContent busy={executing} busyLabel="Deleting…" word="cleaning">
          <Trash2 size={14} aria-hidden="true" />{expired ? 'Expired' : confirmLabel}
        </LoadingActionContent>
      </Button>
    </div>
  </div>
  {#if executing}
    <p class="text-meta font-medium text-foreground">
      <span role="status">Permanently deleting {countLabel(plan.item_count, 'item')}…</span>
      {#if executingSince !== null}<span class="text-muted-foreground"> · <ElapsedTime since={executingSince} /></span>{/if}
    </p>
  {/if}
  {@render children?.()}
  {#if expired}
    <div role="alert" class="flex gap-2 rounded-lg border status-chip-destructive p-2.5 text-meta text-destructive {compact ? 'flex-col' : 'items-center justify-between'}">
      <span>{expiredMessage}</span>
      <div class="flex flex-wrap gap-1.5">
        <Button id={expiryActionId} variant="ghost" size="sm" onclick={onExpiryAction}>{expiryActionLabel}</Button>
        {#if onDismiss}<Button variant="ghost" size="sm" onclick={onDismiss} class="text-destructive">Dismiss</Button>{/if}
      </div>
    </div>
  {:else}
    <p class="text-meta text-muted-foreground">
      {showExpiryTime ? `One-shot, expires at ${expiresAt} (${formatCountdown(remainingSecs)}). ${note}` : note}
    </p>
    {@render footer?.()}
  {/if}
</Card>
