import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import NeatiWordmark from '../lib/components/NeatiWordmark.svelte';

const read = (path: string) => readFileSync(new URL(`../../${path}`, import.meta.url), 'utf8');

describe('Neati identity continuity', () => {
  it('changes display identity without moving persistent data or credentials', () => {
    const config = JSON.parse(read('src-tauri/tauri.conf.json'));
    expect(config.productName).toBe('Neati');
    expect(config.identifier).toBe('com.zenith.desktop');
    expect(config.app.windows.map((w: { title: string }) => w.title)).toEqual(['Neati', 'Neati Quick']);
    const credentials = read('src-tauri/src/ai_providers/credentials.rs');
    expect(credentials).toContain('app.zenith.ai.{provider}');
    expect(credentials).toContain('ZenithAI:{provider}');
    expect(read('src-tauri/src/diagnostics/mod.rs')).toContain('Library/Logs/Zenith');
    expect(read('src-tauri/Cargo.toml')).toContain('name = "Neati"');
    expect(read('src-tauri/Cargo.toml')).toContain('name = "zenith-desktop"');
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

  it('refuses legacy Windows installs without executing an uninstall command', () => {
    const config = JSON.parse(read('src-tauri/tauri.conf.json'));
    expect(config.bundle.windows.nsis.installerHooks).toBe('windows/rebrand-hooks.nsh');
    const hook = read('src-tauri/windows/rebrand-hooks.nsh');
    expect(hook).toContain('ReadRegStr $R0 HKCU');
    expect(hook).toContain('ReadRegStr $R1 HKLM');
    expect(hook).toContain('SetErrorLevel 2');
    expect(hook).toContain('Abort');
    expect(hook).not.toMatch(/ExecWait|ExecShell|DeleteRegKey|RMDir/);
  });

  it('exposes one accessible product name without a downloaded handwriting font', () => {
    const body = render(NeatiWordmark).body;
    expect(body).toContain('Neati</span>');
    expect(body).toContain('aria-hidden="true"');
    expect(body).not.toContain('<text');
    expect(body).not.toContain('@font-face');
  });
});
