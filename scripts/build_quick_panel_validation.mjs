import { build } from 'vite';
import { fileURLToPath } from 'node:url';

const fixture = fileURLToPath(new URL('../src/test/fixtures/quickWindowMock.ts', import.meta.url));
// Aliases are confined to this validation build; production IPC is unchanged.
await build({
  plugins: [{
    name: 'quick-window-validation',
    enforce: 'pre',
    resolveId(source) {
      if (['@tauri-apps/api/core', '@tauri-apps/api/window', '@tauri-apps/api/webviewWindow'].includes(source)) return fixture;
    },
  }],
  build: {
    outDir: '/private/tmp/neati-quick-panel-validation',
    emptyOutDir: true,
    rollupOptions: { input: 'src/test/fixtures/quick-panel.html' },
  },
});
