<script lang="ts">
  import { tick } from 'svelte';
  import LoadingIndicator from '../../lib/components/LoadingIndicator.svelte';
  import LoadingActionContent from '../../lib/components/LoadingActionContent.svelte';
  import type { LoadingWord, LoadingTone, LoadingMotion } from '../../lib/utils/handwrittenStatus';
  let active = $state(true);
  let tone = $state<LoadingTone>('brand');
  let motion = $state<LoadingMotion>('write');
  const words: LoadingWord[] = ['loading', 'scanning', 'cleaning', 'working'];
  const sizes = ['xs', 'sm', 'md'] as const;
  export async function configure(next: { active?: boolean; tone?: LoadingTone; motion?: LoadingMotion; dark?: boolean }) {
    if (next.active !== undefined) active = next.active;
    if (next.tone) tone = next.tone;
    if (next.motion) motion = next.motion;
    if (next.dark !== undefined) document.documentElement.classList.toggle('dark', next.dark);
    await tick();
  }
</script>
<main class="mx-auto max-w-3xl p-6 text-foreground">
  <h1 class="text-title font-semibold">neati working indicators</h1>
  <p class="mt-1 text-body text-muted-foreground">Mounted production components · synthetic operations · v{__APP_VERSION__}</p>
  <div class="mt-6 space-y-4">
    <section class="rounded-xl border border-border bg-card p-4" aria-label="Three-dot loading">
      <p class="text-meta text-muted-foreground">Default three-dot loading</p>
      <div class="mt-2 flex flex-wrap items-center gap-6">
        {#each sizes as size}
          <div class="flex items-center gap-2" data-dot-slot={size}>
            <LoadingIndicator {size} {active} />
            <span class="text-caption text-muted-foreground">{size}</span>
          </div>
        {/each}
      </div>
    </section>
    {#each words as word}
      <section class="rounded-xl border border-border bg-card p-4" aria-label={word}>
        <p class="text-meta text-muted-foreground">{word}</p>
        <div class="mt-2 flex flex-wrap items-center gap-6">
          {#each sizes as size}
            <div class="flex items-center gap-2" data-word-slot={word} data-size-slot={size}>
              <LoadingIndicator variant="handwriting" {word} {size} {tone} {motion} {active} />
              <span class="text-caption text-muted-foreground">{size}</span>
            </div>
          {/each}
        </div>
      </section>
    {/each}
  </div>
  <div class="mt-6 flex flex-wrap gap-3">
    <button class="rounded-lg border border-border-strong bg-card px-3 py-1.5 text-body" aria-label="Refresh usage" disabled={active} data-action-proof>
      <LoadingActionContent busy={active} busyLabel="Refreshing usage" word="loading" {tone} {motion}>
        Refresh usage
      </LoadingActionContent>
    </button>
  </div>
</main>
