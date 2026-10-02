/** One real system-wide CPU observation, never an interpolated value. */
export interface CpuSample {
  at: number;
  percent: number;
}

export const CPU_HISTORY_WINDOW_MS = 150_000;
export const CPU_HISTORY_LIMIT = 60;
/** Foreground collector cadence; a measurement window can be longer after a pause. */
export const CPU_HISTORY_EXPECTED_INTERVAL_MS = 2500;
export const CPU_HISTORY_GAP_FACTOR = 2.5;

/** Bound history by elapsed time as well as count, including failed refreshes. */
export function recentCpuSamples(samples: readonly CpuSample[], endAt: number): CpuSample[] {
  return samples.filter(sample =>
    Number.isFinite(sample.at) && Number.isFinite(sample.percent) &&
    sample.at >= endAt - CPU_HISTORY_WINDOW_MS && sample.at <= endAt &&
    sample.percent >= 0 && sample.percent <= 100
  ).slice(-CPU_HISTORY_LIMIT);
}
