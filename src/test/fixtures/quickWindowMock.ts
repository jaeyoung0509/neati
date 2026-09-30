/** Browser-only window port for mounting the real panel. Unknown IPC fails closed. */
type Monitor = { scaleFactor: number; workArea: { size: { height: number } } };
type Listener = (event: { payload: boolean }) => void;
const focus = new Set<Listener>();
const move = new Set<() => void>();
export const windowProbe = {
  visible: true,
  maximum: 740,
  requests: [] as Array<{ width: number; height: number }>,
  monitorRequests: 0,
  deferred: null as Promise<Monitor> | null,
  setVisible(visible: boolean) {
    this.visible = visible;
    focus.forEach(listener => listener({ payload: visible }));
  },
  viewport(width: number, maximum: number) {
    this.maximum = maximum;
    document.getElementById('app')!.style.width = `${width}px`;
    move.forEach(listener => listener());
  },
};
function monitor(): Monitor {
  return { scaleFactor: 2, workArea: { size: { height: (windowProbe.maximum + 24) * 2 } } };
}
export async function currentMonitor() {
  windowProbe.monitorRequests += 1;
  const pending = windowProbe.deferred;
  windowProbe.deferred = null;
  return pending ? await pending : monitor();
}
const current = {
  async setSize(size: { width: number; height: number }) {
    windowProbe.requests.push({ ...size });
    const app = document.getElementById('app')!;
    app.style.width = `${size.width}px`;
    app.style.height = `${size.height}px`;
  },
  async onMoved(listener: () => void) { move.add(listener); return () => { move.delete(listener); }; },
  async onScaleChanged(listener: () => void) { move.add(listener); return () => { move.delete(listener); }; },
  async onFocusChanged(listener: Listener) { focus.add(listener); return () => { focus.delete(listener); }; },
  async isVisible() { return windowProbe.visible; },
  async hide() { windowProbe.setVisible(false); },
  async setTheme() {},
  async startDragging() {},
};
export const getCurrentWebviewWindow = () => current;
export const getCurrentWindow = () => current;
export class Channel<T> { onmessage: ((value: T) => void) | undefined; }
export async function invoke<T>(command: string): Promise<T> {
  if (command === 'get_agent_quick_summary') return { active_count: 0, attention_count: 0, sessions: [] } as T;
  throw new Error(`Validation fixture refused native command: ${command}`);
}
