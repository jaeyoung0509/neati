import { afterEach, describe, expect, it, vi } from 'vitest';
import { observeWhileVisible } from '../lib/utils/visiblePolling';

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

function page(visibilityState: 'visible' | 'hidden') {
  const document = Object.assign(new EventTarget(), { visibilityState });
  vi.stubGlobal('document', document);
  return document;
}

describe('visible route work', () => {
  it('does no hidden work and refreshes immediately on return without duplicate timers', () => {
    vi.useFakeTimers();
    const document = page('hidden');
    const read = vi.fn();
    const release = observeWhileVisible(read, 5000);
    vi.advanceTimersByTime(15_000);
    expect(read).not.toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(0);

    document.visibilityState = 'visible';
    document.dispatchEvent(new Event('visibilitychange'));
    document.dispatchEvent(new Event('visibilitychange'));
    expect(read).toHaveBeenCalledTimes(1);
    expect(vi.getTimerCount()).toBe(1);
    vi.advanceTimersByTime(10_000);
    expect(read).toHaveBeenCalledTimes(3);

    document.visibilityState = 'hidden';
    document.dispatchEvent(new Event('visibilitychange'));
    vi.advanceTimersByTime(15_000);
    expect(read).toHaveBeenCalledTimes(3);
    expect(vi.getTimerCount()).toBe(0);
    document.visibilityState = 'visible';
    document.dispatchEvent(new Event('visibilitychange'));
    expect(read).toHaveBeenCalledTimes(4);
    release();
    release();
    document.dispatchEvent(new Event('visibilitychange'));
    vi.advanceTimersByTime(15_000);
    expect(read).toHaveBeenCalledTimes(4);
    expect(vi.getTimerCount()).toBe(0);
  });

  it('repeated release cannot stop another consumer or revive an old activation', () => {
    vi.useFakeTimers();
    const document = page('visible');
    const first = vi.fn();
    const second = vi.fn();
    const releaseFirst = observeWhileVisible(first, 1000);
    const releaseSecond = observeWhileVisible(second, 1000);
    releaseFirst();
    releaseFirst();
    vi.advanceTimersByTime(1000);
    expect(first).toHaveBeenCalledTimes(1);
    expect(second).toHaveBeenCalledTimes(2);

    document.visibilityState = 'hidden';
    document.dispatchEvent(new Event('visibilitychange'));
    document.visibilityState = 'visible';
    document.dispatchEvent(new Event('visibilitychange'));
    expect(first).toHaveBeenCalledTimes(1);
    expect(second).toHaveBeenCalledTimes(3);
    releaseSecond();
    expect(vi.getTimerCount()).toBe(0);
  });
});
