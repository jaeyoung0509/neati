import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';

const fixture = fileURLToPath(new URL('../src/test/fixtures/quickWindowMock.ts', import.meta.url));
const output = process.argv[2] ?? '/private/tmp/neati-quick-panel-validation';
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
    outDir: output,
    emptyOutDir: true,
    assetsInlineLimit: Infinity,
    rollupOptions: {
      input: 'src/test/fixtures/quick-panel.html',
      output: { inlineDynamicImports: true },
    },
  },
});
// Also retain the normal asset tree for the existing HTTP validation script.
// The extra standalone entry lets an isolated browser review the same mounted
// panel offline, including the real production loading/motion components.
const entry = resolve(output, 'src/test/fixtures/quick-panel.html');
let html = readFileSync(entry, 'utf8');
html = html.replace(/<script[^>]+src="([^"]+)"[^>]*><\/script>/g, (_tag, path) => {
  let script = readFileSync(resolve(dirname(entry), path), 'utf8');
  for (const asset of readdirSync(resolve(output, 'assets'))) {
    const type = asset.endsWith('.svg') ? 'image/svg+xml' : asset.endsWith('.png') ? 'image/png' : null;
    if (type) script = script.replaceAll(asset, `data:${type};base64,${readFileSync(resolve(output, 'assets', asset)).toString('base64')}`);
  }
  return `<script type="module">${script.replace(/<\/script/gi, '<\\/script')}</script>`;
});
html = html.replace(/<link[^>]+rel="stylesheet"[^>]+href="([^"]+)"[^>]*>/g, (_tag, path) =>
  `<style>${readFileSync(resolve(dirname(entry), path), 'utf8')}</style>`);
mkdirSync(output, { recursive: true });
writeFileSync(resolve(output, 'quick-panel.html'), html);
console.log(`Offline mounted Quick Panel fixture: ${resolve(output, 'quick-panel.html')}`);
