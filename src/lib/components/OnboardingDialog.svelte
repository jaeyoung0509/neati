<script lang="ts">
  import Button from './Button.svelte';
  import Dialog from './Dialog.svelte';
  import LaunchAtLoginSetting from './LaunchAtLoginSetting.svelte';
  import StorageAccessSetup from './StorageAccessSetup.svelte';
  import { ONBOARDING_STEPS } from '../utils/onboarding';

  /** `onFinish` runs for Done and for dismissal: both mean "do not show again". */
  let { onFinish }: { onFinish: () => void } = $props();
  const id = $props.id();
  let index = $state(0);
  let step = $derived(ONBOARDING_STEPS[index]);
  let isLast = $derived(index === ONBOARDING_STEPS.length - 1);
  let heading = $state<HTMLHeadingElement>();

  function go(next: number) {
    index = next;
    // Move focus to the new heading so the step change is announced.
    queueMicrotask(() => heading?.focus());
  }
</script>

<Dialog onCancel={onFinish} initialFocus="h2" size="lg" aria-labelledby={id + '-title'} aria-describedby={id + '-body'} data-onboarding>
  <p class="text-caption text-muted-foreground" aria-live="polite">Step {index + 1} of {ONBOARDING_STEPS.length}</p>
  <h2 bind:this={heading} id={id + '-title'} tabindex="-1" class="mt-1 text-title font-semibold focus:outline-none">{step.title}</h2>
  <div id={id + '-body'} class="mt-3 space-y-3 text-body text-muted-foreground">
    {#each step.paragraphs as paragraph}<p>{paragraph}</p>{/each}
    {#if step.points}
      <ul class="list-disc space-y-1 pl-5">
        {#each step.points as point}<li>{point}</li>{/each}
      </ul>
    {/if}
  </div>

  {#if step.id === 'access'}
    <div class="mt-4 space-y-4 rounded-xl border border-border bg-secondary p-3">
      <div class="space-y-1 text-meta">
        <div class="font-medium text-foreground">Full Disk Access</div>
        <div class="text-muted-foreground">Lets neati measure protected locations such as Mail and app containers. Scans work without it, with those locations left out.</div>
        <StorageAccessSetup compact />
      </div>
      <LaunchAtLoginSetting />
    </div>
  {/if}

  <div class="mt-5 flex items-center justify-between gap-2">
    <Button variant="ghost" size="sm" onclick={onFinish}>Skip</Button>
    <div class="flex gap-2">
      {#if index > 0}<Button variant="outline" onclick={() => go(index - 1)}>Back</Button>{/if}
      {#if isLast}
        <Button onclick={onFinish}>Done</Button>
      {:else}
        <Button onclick={() => go(index + 1)}>Next</Button>
      {/if}
    </div>
  </div>
</Dialog>
