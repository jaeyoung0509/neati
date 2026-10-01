<script lang="ts">
  import CleanupReviewDialog from '../../lib/components/CleanupReviewDialog.svelte';
  import CleanupQuitDialog from '../../lib/components/CleanupQuitDialog.svelte';
  import CleanResultModal from '../../lib/components/CleanResultModal.svelte';
  import QuickCleanupDetailsDialog from '../../lib/components/QuickCleanupDetailsDialog.svelte';
  import QuickSafeReviewDialog from '../../lib/components/QuickSafeReviewDialog.svelte';
  import type { PlanPreview, ScanItem, CleanResult, CleanupQuitPreview, ScanResult } from '../../lib/models/types';
  let { plan, items, result, quit, scan }: {
    plan: PlanPreview; items: ScanItem[]; result: CleanResult; quit: CleanupQuitPreview; scan: ScanResult;
  } = $props();
  let kind = $state<string | null>(null);
  export function show(value: string) { kind = value; }
  export function dismiss() { kind = null; }
</script>

{#if kind === 'review'}
  <CleanupReviewDialog {plan} {items} onCancel={dismiss} onConfirm={dismiss} />
{:else if kind === 'quit'}
  <CleanupQuitDialog preview={quit} busy={false} progress="" onCancel={dismiss} onConfirm={dismiss} />
{:else if kind === 'result'}
  <CleanResultModal {result} onClose={dismiss} />
{:else if kind === 'details'}
  <QuickCleanupDetailsDialog {scan} quickEligibleCount={3} onClose={dismiss} onReview={dismiss} onRescan={dismiss} />
{:else if kind === 'quick-review'}
  <QuickSafeReviewDialog {items} partial={true} onCancel={dismiss} onConfirm={dismiss} />
{/if}
