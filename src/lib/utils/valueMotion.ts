/** Values are already rendered by Svelte. This only reveals the latest reading. */
export function createValueMotion(element: HTMLElement, now = () => performance.now()) {
  let previous: string | undefined;
  let lastAnimated = -Infinity;
  let animation: Animation | undefined;
  const stop = () => { animation?.cancel(); animation = undefined; };
  return {
    update(value: string, allowed: boolean) {
      if (previous === value) {
        if (!allowed) stop();
        return;
      }
      const hadValue = previous !== undefined;
      previous = value;
      stop();
      const timestamp = now();
      // Bursts replace immediately, without queued transitions or frame loops.
      if (!hadValue || !allowed || !/\d/.test(value) || timestamp - lastAnimated < 240 || !element.animate) return;
      lastAnimated = timestamp;
      animation = element.animate(
        [{ transform: 'translateY(5px)', opacity: 0.65 }, { transform: 'translateY(0)', opacity: 1 }],
        { duration: 180, easing: 'cubic-bezier(0.215, 0.61, 0.355, 1)' },
      );
    },
    stop,
  };
}
