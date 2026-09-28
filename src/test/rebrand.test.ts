import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import NeatiWordmark from '../lib/components/NeatiWordmark.svelte';

const read = (path: string) => readFileSync(new URL(`../../${path}`, import.meta.url), 'utf8');

describe('Neati independent pre-release identity', () => {
  it('uses Neati namespaces without importing legacy data or credentials', () => {
    const config = JSON.parse(read('src-tauri/tauri.conf.json'));
    expect(config.productName).toBe('Neati');
    expect(config.identifier).toBe('com.neati.desktop');
    expect(config.app.windows.map((w: { title: string }) => w.title)).toEqual(['Neati', 'Neati Quick']);
    const credentials = read('src-tauri/src/ai_providers/credentials.rs');
    expect(credentials).toContain('app.neati.ai.{provider}');
    expect(credentials).toContain('NeatiAI:{provider}');
    expect(read('src-tauri/src/diagnostics/mod.rs')).toContain('Library/Logs/Neati');
    expect(read('src-tauri/Cargo.toml')).toContain('name = "Neati"');
    expect(read('src-tauri/Cargo.toml')).toContain('name = "neati-desktop"');
    expect(read('src-tauri/Cargo.toml')).toContain('name = "neati_lib"');
    expect(read('Cargo.toml')).toContain('"crates/neati-core"');
    expect(read('Cargo.toml')).toContain('"crates/neati-platform"');
    expect(JSON.parse(read('package.json')).name).toBe('neati');
    expect(read('src-tauri/src/ai_providers/credentials.rs')).toContain('"neati"');
    expect(read('scripts/install_release_app.sh')).toContain('expected_bundle_id="com.neati.desktop"');
    expect(read('scripts/install_release_app.sh')).not.toContain('previous Neati.app');
  });

  it('keeps the approved B silhouette identical in the monochrome template', () => {
    const master = read('src-tauri/icons/neati-mark.svg');
    const template = read('src-tauri/icons/tray-icon.svg');
    const path = /<path d="([^"]+)"/.exec(master)?.[1];
    expect(path).toBeTruthy();
    expect(template).toContain(`d="${path}"`);
    expect(template).toContain('stroke="#000"');
    expect(template).toContain('stroke-linecap="round"');
    expect(master).toContain('stroke="#fff"');
  });

  it('does not require or execute a legacy Windows uninstall', () => {
    const config = JSON.parse(read('src-tauri/tauri.conf.json'));
    expect(config.bundle.windows.nsis.installerHooks).toBeUndefined();
    expect(read('scripts/windows_packaging_smoke.ps1')).not.toContain('legacyKey');
  });

  it('exposes one accessible product name without a downloaded handwriting font', () => {
    const body = render(NeatiWordmark).body;
    expect(body).toContain('Neati</span>');
    expect(body).toContain('aria-hidden="true"');
    expect(body).not.toContain('<text');
    expect(body).not.toContain('@font-face');
  });
});
