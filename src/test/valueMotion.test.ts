// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createValueMotion } from '../lib/utils/valueMotion';
import { observeMotion } from '../lib/utils/motionVisibility';

afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); vi.useRealTimers(); });

describe('bounded value feedback', () => {
  it('keeps the exact reading and cancels rather than queuing burst updates', () => {
    const element = document.createElement('span');
    const cancellations: ReturnType<typeof vi.fn>[] = [];
    element.animate = vi.fn(() => {
      const cancel = vi.fn();
      cancellations.push(cancel);
      return { cancel } as unknown as Animation;
    });
    let time = 0;
    const controller = createValueMotion(element, () => time);
    controller.update('—', true);
    element.textContent = '1.8 GB';
    controller.update('1.8 GB', true);
    expect(element.animate).toHaveBeenCalledTimes(1);
    expect(element.textContent).toBe('1.8 GB');
    time = 20;
    element.textContent = '2.1 GB';
    controller.update('2.1 GB', true);
    expect(cancellations[0]).toHaveBeenCalledOnce();
    expect(element.animate).toHaveBeenCalledTimes(1);
    expect(element.textContent).toBe('2.1 GB');
    time = 300;
    controller.update('2.2 GB', true);
    expect(element.animate).toHaveBeenCalledTimes(2);
    controller.update('2.2 GB', false);
    expect(cancellations[1]).toHaveBeenCalledOnce();
    controller.stop();
    expect(cancellations[1]).toHaveBeenCalledOnce();
  });

  it('adds no timer and never replays a hidden update when visibility returns', () => {
    vi.useFakeTimers();
    const element = document.createElement('span');
    element.animate = vi.fn();
    const controller = createValueMotion(element);
    controller.update('11.7 GB', false);
    controller.update('12.0 GB', false);
    controller.update('12.0 GB', true);
    controller.update('Unavailable', true);
    expect(element.animate).not.toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(0);
  });
});

describe('subscriber-owned motion visibility', () => {
  it('shares observation, pauses on preferences/visibility, and releases the last subscriber', () => {
    vi.useFakeTimers();
    let hidden = false;
    vi.spyOn(document, 'hidden', 'get').mockImplementation(() => hidden);
    const media = new Map<string, { matches: boolean; callback?: () => void; removeEventListener: ReturnType<typeof vi.fn> }>();
    vi.stubGlobal('matchMedia', (query: string) => {
      const item = { matches: false, callback: undefined as (() => void) | undefined, addEventListener: vi.fn((_type, callback) => { item.callback = callback; }), removeEventListener: vi.fn() };
      media.set(query, item);
      return item;
    });
    let sendEntries: IntersectionObserverCallback;
    const unobserve = vi.fn();
    const disconnect = vi.fn();
    const construction = vi.fn(function(callback: IntersectionObserverCallback) {
      sendEntries = callback;
      return { observe: vi.fn(), unobserve, disconnect };
    });
    vi.stubGlobal('IntersectionObserver', construction);
    const first = document.createElement('span');
    const second = document.createElement('svg');
    const firstUpdates = vi.fn();
    const secondUpdates = vi.fn();
    const stopFirst = observeMotion(first, firstUpdates);
    const stopSecond = observeMotion(second, secondUpdates);
    expect(construction).toHaveBeenCalledOnce();
    expect(firstUpdates).toHaveBeenLastCalledWith(false);
    sendEntries!([first, second].map(target => ({ target, isIntersecting: true, boundingClientRect: new DOMRect(), intersectionRect: new DOMRect(), intersectionRatio: 1, rootBounds: null, time: 0 })), {} as IntersectionObserver);
    expect(firstUpdates).toHaveBeenLastCalledWith(true);
    const reduced = media.get('(prefers-reduced-motion: reduce)')!;
    reduced.matches = true;
    reduced.callback!();
    expect(secondUpdates).toHaveBeenLastCalledWith(false);
    reduced.matches = false;
    const reducedTransparency = media.get('(prefers-reduced-transparency: reduce)')!;
    reducedTransparency.matches = true;
    reducedTransparency.callback!();
    expect(firstUpdates).toHaveBeenLastCalledWith(false);
    expect(secondUpdates).toHaveBeenLastCalledWith(false);
    reducedTransparency.matches = false;
    reducedTransparency.callback!();
    expect(firstUpdates).toHaveBeenLastCalledWith(true);
    sendEntries!([{ target: first, isIntersecting: false, boundingClientRect: new DOMRect(), intersectionRect: new DOMRect(), intersectionRatio: 0, rootBounds: null, time: 1 }], {} as IntersectionObserver);
    expect(firstUpdates).toHaveBeenLastCalledWith(false);
    expect(secondUpdates).toHaveBeenLastCalledWith(true);
    hidden = true;
    document.dispatchEvent(new Event('visibilitychange'));
    expect(firstUpdates).toHaveBeenLastCalledWith(false);
    hidden = false;
    document.dispatchEvent(new Event('visibilitychange'));
    expect(secondUpdates).toHaveBeenLastCalledWith(true);
    expect(firstUpdates).toHaveBeenLastCalledWith(false);
    stopFirst(); stopFirst();
    expect(unobserve).toHaveBeenCalledTimes(1);
    expect(disconnect).not.toHaveBeenCalled();
    stopSecond();
    expect(disconnect).toHaveBeenCalledOnce();
    for (const item of media.values()) expect(item.removeEventListener).toHaveBeenCalledOnce();
    expect(vi.getTimerCount()).toBe(0);
  });
});
