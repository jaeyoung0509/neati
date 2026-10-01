/** One observer per webview; listeners exist only while a visible surface owns them. */
const surfaces = new Map<Element, { intersects: boolean; listeners: Set<(allowed: boolean) => void> }>();
let observer: IntersectionObserver | undefined;
let motion: MediaQueryList | undefined;
let transparency: MediaQueryList | undefined;

function publish() {
  const allowed = !document.hidden && !motion?.matches && !transparency?.matches;
  for (const surface of surfaces.values()) {
    for (const listener of surface.listeners) listener(allowed && surface.intersects);
  }
}

export function observeMotion(element: Element, listener: (allowed: boolean) => void): () => void {
  if (!surfaces.size) {
    motion = window.matchMedia('(prefers-reduced-motion: reduce)');
    transparency = window.matchMedia('(prefers-reduced-transparency: reduce)');
    motion.addEventListener('change', publish);
    transparency.addEventListener('change', publish);
    document.addEventListener('visibilitychange', publish);
    if (typeof IntersectionObserver !== 'undefined') {
      observer = new IntersectionObserver((entries) => {
        for (const entry of entries) {
          const surface = surfaces.get(entry.target);
          if (surface) surface.intersects = entry.isIntersecting;
        }
        publish();
      });
    }
  }
  let surface = surfaces.get(element);
  if (!surface) {
    surface = { intersects: false, listeners: new Set() };
    surfaces.set(element, surface);
    observer?.observe(element);
  }
  surface.listeners.add(listener);
  publish();
  let disposed = false;
  return () => {
    if (disposed) return;
    disposed = true;
    listener(false);
    surface.listeners.delete(listener);
    if (!surface.listeners.size) {
      surfaces.delete(element);
      observer?.unobserve(element);
    }
    if (!surfaces.size) {
      observer?.disconnect();
      observer = undefined;
      motion?.removeEventListener('change', publish);
      transparency?.removeEventListener('change', publish);
      document.removeEventListener('visibilitychange', publish);
      motion = transparency = undefined;
    }
  };
}
