import { quickPanelHeight } from './quickPanel';

interface PanelMeasurement {
  width: number;
  height: number;
  contentHeight: number;
  chromeHeight: number;
}

export interface QuickPanelSizingPort {
  measure: () => PanelMeasurement | null;
  maximumHeight: () => Promise<number>;
  resize: (width: number, height: number) => Promise<void>;
}

/** Fit durable layout once; streamed readings may overflow the scrolling body. */
export function createQuickPanelSizer(port: QuickPanelSizingPort) {
  let active = false;
  let disposed = false;
  let layout = '';
  let revision = 0;
  let fittedLayout = '';
  let timer: ReturnType<typeof setTimeout> | undefined;
  let applying = Promise.resolve();

  function cancel() {
    revision += 1;
    if (timer !== undefined) clearTimeout(timer);
    timer = undefined;
  }

  async function fit(request: number) {
    let maximum = 740;
    try {
      const available = await port.maximumHeight();
      if (Number.isFinite(available)) maximum = Math.max(1, Math.min(740, available));
    } catch {
      // A failed monitor probe retains the configured upper bound.
    }
    await applying;
    if (disposed || !active || request !== revision) return;
    const measured = port.measure();
    if (!measured || measured.width <= 0) return;
    const identity = JSON.stringify([layout, measured.width, maximum]);
    if (identity === fittedLayout) return;
    const height = quickPanelHeight(measured.contentHeight, measured.chromeHeight, maximum);
    if (Math.abs(measured.height - height) < 1) {
      fittedLayout = identity;
      return;
    }
    let succeeded = false;
    applying = port.resize(measured.width, height).then(() => {
      succeeded = true;
    }).catch(() => undefined);
    await applying;
    if (succeeded && active && !disposed && request === revision) fittedLayout = identity;
  }

  function requestFit() {
    if (!active || disposed) return;
    cancel();
    const request = revision;
    timer = setTimeout(() => {
      timer = undefined;
      void fit(request);
    }, 180);
  }

  return {
    configure(visible: boolean, layoutKey: string) {
      if (disposed || (active === visible && layout === layoutKey)) return;
      cancel();
      active = visible;
      layout = layoutKey;
      if (active) requestFit();
    },
    viewportChanged: requestFit,
    dispose() {
      cancel();
      active = false;
      disposed = true;
    },
  };
}
