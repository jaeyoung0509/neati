// Static Svelte fixtures, rendered from the current source with production CSS.
// No IPC, native permission checks, or real directory inventory runs here.
import { createServer } from 'vite';
import { mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
const version = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8')).version;
const output = process.argv[2] ?? join(root, 'docs', 'validation', `temp-cleanup-${version}`);
const assets = join(root, 'dist', 'assets');
const css = readdirSync(assets).filter(name => name.endsWith('.css'))
  .map(name => readFileSync(join(assets, name), 'utf8')).join('\n');
const server = await createServer({ root, server: { middlewareMode: true, hmr: false, watch: null }, appType: 'custom' });
try {
  const { render } = await server.ssrLoadModule('svelte/server');
  const { default: View } = await server.ssrLoadModule('/src/routes/dashboard/StorageView.svelte');
  const { scanStore } = await server.ssrLoadModule('/src/lib/stores/scan.svelte.ts');
  const now = Math.floor(Date.now() / 1000);
  const base = { scan_id: 'synthetic-temp-validation', valid_for_seconds: 300,
    started_at: now - 1, finished_at: now, categories: [], total_bytes: 0,
    safe_bytes: 0, rebuild_bytes: 0, manual_bytes: 0, quality: 'fresh', incomplete_reasons: [], gaps: [] };
  const item = { id: 'synthetic-node-cache', signature_id: 'system.node_compile_temp',
    name: 'Node Temporary Compilation Cache', category: 'system', risk: 'rebuild',
    path: '/fixture/user-temp/node-compile-cache/v26.7.0-arm64-8d7ad2ee-501',
    size: { logical: 1024 * 1024, allocated: 1024 * 1024 }, file_count: 20,
    description: 'Verified compilation cache; Node may rebuild it later.',
    ownership: { owner: 'Node', confidence: 'declared' },
    is_selected: true, exists: true, quality: 'fresh', incomplete_reason: null,
    owner_running: false, last_modified: null, lifecycle_provider_action: true,
    disposition: { eligibility: 'auto_cleanable', reason: null, cleanable_bytes: 1024 * 1024 } };
  const retained = { ...item, id: 'synthetic-workspace', signature_id: 'system.developer_temp',
    name: 'Temporary workspace (kept)', risk: 'manual', path: '/fixture/user-temp/neati-active-pr',
    is_selected: false, lifecycle_provider_action: false,
    ownership: { owner: 'Owner not established', confidence: 'unknown' },
    description: 'Workspace and PR recovery artifacts remain outside cleanup authority.',
    disposition: { eligibility: 'advisory', reason: null, cleanable_bytes: 0 } };
  const category = items => ({ category: 'system', display_name: 'System', items,
    total_bytes: items.reduce((sum, entry) => sum + entry.size.allocated, 0),
    safe_bytes: 0,
    rebuild_bytes: items.filter(entry => entry.risk === 'rebuild').reduce((sum, entry) => sum + entry.size.allocated, 0),
    manual_bytes: items.filter(entry => entry.risk === 'manual').reduce((sum, entry) => sum + entry.size.allocated, 0) });
  const sized = (entry, id, bytes, disposition, extra = {}) => ({ ...entry, id, name: id,
    size: { logical: bytes, allocated: bytes }, disposition, ...extra });
  const ready = sized(item, 'Verified cache', Math.round(1.8 * 1024 ** 3), { eligibility: 'auto_cleanable', reason: null, cleanable_bytes: Math.round(1.8 * 1024 ** 3) });
  const running = sized(item, 'Cache of an active app', Math.round(2.6 * 1024 ** 3), { eligibility: 'reviewable', reason: 'Quit the owner and scan again.', cleanable_bytes: Math.round(2.6 * 1024 ** 3) }, { owner_running: true });
  const reviewed = sized(item, 'Reviewed owner cleanup', Math.round(443.8 * 1024 ** 2), { eligibility: 'reviewable', reason: 'This owner action requires review.', cleanable_bytes: Math.round(443.8 * 1024 ** 2) });
  const unestimated = sized(item, 'Tool-managed store', 1024 ** 3, { eligibility: 'reviewable', reason: 'The tool decides what is unused.', cleanable_bytes: null }, {
    cache_metadata: { provider: 'pnpm', management_mode: 'tool_managed', artifact_kind: 'package_store', consequence: 'Packages may be downloaded again.', size_semantics: 'informational', last_used_confidence: 'unknown' },
  });
  const diagnosticRows = Array.from({ length: 787 }, (_, index) => sized(retained, `Protected observation ${index + 1}`, 0, { eligibility: 'blocked', reason: 'This observation grants no cleanup authority.', cleanable_bytes: 0 }, { quality: 'unavailable', incomplete_reason: 'Could not inspect this fixture.' }));
  const mixedItems = [ready, running, reviewed, unestimated, ...diagnosticRows];
  const mixed = { ...base, quality: 'partial', categories: [category(mixedItems)],
    total_bytes: Math.round(34.4 * 1024 ** 3), ambiguous_overlap_bytes: Math.round(34.4 * 1024 ** 3) - Math.round(33.2 * 1024 ** 2),
    gaps: [{ kind: 'unknown', count: 8 }, { kind: 'safety_protected', count: 10 }, { kind: 'full_disk_access', count: 459 }, { kind: 'permission_denied', count: 34 }] };
  const scenarios = [
    ['initial', 'Before the first scan', null],
    ['empty', 'Verified empty scan', { ...base }],
    ['unavailable', 'Inspection unavailable; bytes unknown', { ...base, quality: 'unavailable',
      gaps: [{ kind: 'tool_missing', count: 1 }] }],
    ['privacy', 'Protected-path access refusal; bytes unknown', { ...base, quality: 'unavailable',
      gaps: [{ kind: 'full_disk_access', count: 3 }] }],
    ['available', 'Verified Node cache ready for recoverable cleanup', { ...base,
      categories: [category([item])], total_bytes: 1024 * 1024, rebuild_bytes: 1024 * 1024 }],
    ['partial', 'Verified Node cache with incomplete coverage', { ...base, quality: 'partial',
      categories: [category([item, retained])], total_bytes: 2 * 1024 * 1024,
      rebuild_bytes: 1024 * 1024, manual_bytes: 1024 * 1024,
      gaps: [{ kind: 'unknown', count: 1 }] }],
    ['retained', 'Observed workspace; nothing ready to clean', { ...base,
      categories: [category([retained])], total_bytes: 1024 * 1024, manual_bytes: 1024 * 1024 }],
    ['mixed', 'Partial scan with 790 retained/review observations', mixed],
  ];
  mkdirSync(output, { recursive: true });
  writeFileSync(join(output, 'mixed-fixture.json'), JSON.stringify(mixed));
  for (const [name, label, scan] of scenarios) {
    scanStore.lastScan = scan;
    scanStore.error = null;
    scanStore.isScanning = false;
    scanStore.isCleaning = false;
    scanStore.discovery = { status: 'exhausted' };
    scanStore.selectedMap = {};
    if (scan) scanStore.syncSelectionFromScan(scan);
    scanStore.updateFreshness();
    const rendered = render(View, { props: { onSelectCategory: () => {} } });
    for (const dark of [false, true]) {
      const filename = `${name}-${dark ? 'dark' : 'light'}.html`;
      const html = `<!doctype html><html${dark ? ' class="dark"' : ''}><head><meta charset="UTF-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>${label} — neati ${version}</title><style>${css}\nhtml,body{height:auto;min-height:0}body{margin:0}main{max-width:1120px;margin:auto;padding:24px}.validation-label{margin-bottom:16px}</style></head><body class="bg-background text-foreground"><main><p class="validation-label text-meta text-muted-foreground">${label} · neati ${version} · Synthetic data; static controls</p>${rendered.body}</main></body></html>`;
      writeFileSync(join(output, filename), html);
    }
  }
  console.log(`Rendered ${scenarios.length * 2} labeled static fixtures at ${output}`);
} finally {
  await server.close();
}
