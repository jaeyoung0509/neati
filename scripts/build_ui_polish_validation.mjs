// Production Svelte components in a self-contained, synthetic browser fixture.
// Inline modules allow offline file:// review when a local server is unavailable.
import { build } from 'vite';
import { mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';

const output = process.argv[2] ?? '/private/tmp/neati-ui-polish-validation';
await build({
  build: {
    outDir: output,
    emptyOutDir: true,
    assetsInlineLimit: Infinity,
    rollupOptions: {
      input: 'src/test/fixtures/ui-polish.html',
      output: { inlineDynamicImports: true },
    },
  },
});
const entry = resolve(output, 'src/test/fixtures/ui-polish.html');
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
writeFileSync(resolve(output, 'ui-polish.html'), html);
console.log(`Offline mounted UI fixture: ${resolve(output, 'ui-polish.html')}`);
