<script lang="ts">
  import { CPU_HISTORY_WINDOW_MS, CPU_HISTORY_EXPECTED_INTERVAL_MS, CPU_HISTORY_GAP_FACTOR, recentCpuSamples, type CpuSample } from '../../utils/cpuHistory';

  let { samples, endAt, expectedIntervalMs = CPU_HISTORY_EXPECTED_INTERVAL_MS, compact = false }: {
    samples: CpuSample[];
    endAt: number;
    expectedIntervalMs?: number;
    compact?: boolean;
  } = $props();

  let recent = $derived(recentCpuSamples(samples, endAt));
  let runs = $derived.by(() => {
    const result: CpuSample[][] = [];
    for (const sample of recent) {
      const run = result[result.length - 1];
      if (!run || sample.at - run[run.length - 1].at > expectedIntervalMs * CPU_HISTORY_GAP_FACTOR) {
        result.push([sample]);
      } else run.push(sample);
    }
    return result;
  });
  let latest = $derived(recent[recent.length - 1]);
  let description = $derived(recent.length
    ? `CPU history over the last 2 minutes 30 seconds, on a 0 to 100 percent scale. ${recent.length} recorded ${recent.length === 1 ? 'sample' : 'samples'}. Most recent: ${latest.percent.toFixed(1)} percent. ${runs.length > 1 ? `${runs.length - 1} recording ${runs.length === 2 ? 'gap' : 'gaps'}; missing readings are not connected.` : ''}`
    : 'No CPU readings recorded in the last 2 minutes 30 seconds.');

  function pointX(at: number): number {
    return 1 + ((at - (endAt - CPU_HISTORY_WINDOW_MS)) / CPU_HISTORY_WINDOW_MS) * 98;
  }
  function pointY(percent: number): number { return 4 + (100 - percent) * 1.04; }
  function points(run: CpuSample[]): string {
    return run.map(sample => `${pointX(sample.at).toFixed(3)},${pointY(sample.percent).toFixed(3)}`).join(' ');
  }
</script>

<div class="grid gap-x-2 gap-y-1.5 {compact ? 'grid-cols-1' : 'grid-cols-[2.5rem_minmax(0,1fr)]'}" data-cpu-history>
  {#if !compact}
  <div class="flex h-28 flex-col justify-between text-right font-mono text-caption text-muted-foreground" aria-hidden="true">
    <span>100%</span><span>50%</span><span>0%</span>
  </div>
  {/if}
  <div class="relative min-w-0 {compact ? 'h-8' : 'h-28'}">
    <svg class="h-full w-full overflow-visible" viewBox="0 0 100 112" preserveAspectRatio="none" role="img" aria-label={description}>
      {#each [4, 56, 108] as y}
        <line x1="1" x2="99" y1={y} y2={y} stroke="hsl(var(--border))" stroke-width="1" vector-effect="non-scaling-stroke" />
      {/each}
      {#each runs as run}
        {#if run.length > 1}
          <polyline points={points(run)} fill="none" stroke="hsl(var(--primary))" stroke-width="2" stroke-linejoin="round" stroke-linecap="round" vector-effect="non-scaling-stroke" />
        {/if}
        {#if run.length === 1 || run[run.length - 1] === latest}
          {@const point = run[run.length - 1]}
          <!-- A round, non-scaling stroke stays circular when the SVG stretches. -->
          <line x1={pointX(point.at)} x2={pointX(point.at)} y1={pointY(point.percent)} y2={pointY(point.percent)} stroke="hsl(var(--primary))" stroke-width="5" stroke-linecap="round" vector-effect="non-scaling-stroke" />
        {/if}
      {/each}
    </svg>
    {#if !recent.length && !compact}
      <p class="absolute inset-0 flex items-center justify-center text-meta text-muted-foreground">No readings in this window</p>
    {/if}
  </div>
  {#if !compact}
  <div></div>
  <div class="flex justify-between font-mono text-caption text-muted-foreground" aria-hidden="true">
    <span>2m 30s ago</span><span>1m 15s ago</span><span>Now</span>
  </div>
  {/if}
</div>
