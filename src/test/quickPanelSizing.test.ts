import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createQuickPanelSizer } from '../lib/utils/quickPanelSizing';

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

function harness() {
  const measured = { width: 360, height: 500, contentHeight: 448, chromeHeight: 122 };
  const resize = vi.fn(async (_width: number, height: number) => { measured.height = height; });
  const maximumHeight = vi.fn(async () => 740);
  const sizer = createQuickPanelSizer({ measure: () => measured, maximumHeight, resize });
  return { measured, resize, maximumHeight, sizer };
}

describe('Quick Panel durable bounds', () => {
  it('fits once across streamed gauges, stale/error text and repeated activations', async () => {
    const { measured, resize, sizer } = harness();
    sizer.configure(true, 'standard:codex,antigravity');
    await vi.advanceTimersByTimeAsync(180);
    expect(resize.mock.calls).toEqual([[360, 570]]);
    for (const height of [460, 471, 448, 525, 471]) {
      measured.contentHeight = height;
      sizer.viewportChanged();
      await vi.advanceTimersByTimeAsync(180);
    }
    sizer.configure(false, '');
    sizer.configure(true, 'standard:codex,antigravity');
    await vi.advanceTimersByTimeAsync(180);
    expect(resize).toHaveBeenCalledTimes(1);
    sizer.dispose();
  });

  it('refits deliberately for preferences, width and constrained display changes', async () => {
    const { measured, resize, maximumHeight, sizer } = harness();
    sizer.configure(true, 'standard');
    await vi.advanceTimersByTimeAsync(180);
    measured.contentHeight = 180;
    sizer.configure(true, 'cpu,memory');
    await vi.advanceTimersByTimeAsync(180);
    expect(resize).toHaveBeenLastCalledWith(360, 302);
    measured.width = 320;
    measured.contentHeight = 700;
    maximumHeight.mockResolvedValue(420);
    sizer.viewportChanged();
    await vi.advanceTimersByTimeAsync(180);
    expect(resize).toHaveBeenLastCalledWith(320, 420);
    expect(resize).toHaveBeenCalledTimes(3);
    sizer.dispose();
  });

  it('rejects an old monitor measurement after hiding and reopening', async () => {
    const { resize, maximumHeight, sizer } = harness();
    let finishOld: (height: number) => void = () => {};
    maximumHeight.mockImplementationOnce(() => new Promise(resolve => { finishOld = resolve; }));
    sizer.configure(true, 'old-preferences');
    await vi.advanceTimersByTimeAsync(180);
    sizer.configure(false, '');
    sizer.configure(true, 'new-preferences');
    await vi.advanceTimersByTimeAsync(180);
    expect(resize.mock.calls).toEqual([[360, 570]]);
    finishOld(320);
    await vi.advanceTimersByTimeAsync(0);
    expect(resize).toHaveBeenCalledTimes(1);
    sizer.dispose();
  });

  it('cancels pending work on disposal and does no hidden monitor probing', async () => {
    const { resize, maximumHeight, sizer } = harness();
    sizer.configure(true, 'standard');
    sizer.dispose();
    sizer.viewportChanged();
    sizer.configure(true, 'new');
    await vi.runAllTimersAsync();
    expect(maximumHeight).not.toHaveBeenCalled();
    expect(resize).not.toHaveBeenCalled();
  });

  it('serializes native calls so a newer layout applies after an in-flight resize', async () => {
    const { measured, resize, sizer } = harness();
    let finishFirst: () => void = () => {};
    resize.mockImplementationOnce(() => new Promise<void>(resolve => { finishFirst = resolve; }));
    sizer.configure(true, 'first');
    await vi.advanceTimersByTimeAsync(180);
    measured.contentHeight = 200;
    sizer.configure(true, 'second');
    await vi.advanceTimersByTimeAsync(180);
    expect(resize).toHaveBeenCalledTimes(1);
    finishFirst();
    await vi.advanceTimersByTimeAsync(0);
    expect(resize.mock.calls).toEqual([[360, 570], [360, 322]]);
    sizer.dispose();
  });
});
