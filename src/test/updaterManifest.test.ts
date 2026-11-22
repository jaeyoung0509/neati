import { execFileSync, spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

const script = 'scripts/updater_manifest.cjs';

function fixture(signature = 'untrusted comment: signature\nRWQ...\ntrusted comment: timestamp:1\tfile:neati.app.tar.gz\tversion:0.3.111\nabc\n') {
  const dir = mkdtempSync(join(tmpdir(), 'neati-updater-'));
  const sig = join(dir, 'neati.app.tar.gz.sig');
  writeFileSync(sig, signature);
  return { dir, sig, output: join(dir, 'latest.json') };
}

describe('updater manifest script', () => {
  it('binds the version, signature and the tagged release asset URL', () => {
    const { sig, output } = fixture();
    execFileSync('node', [script, '--version', '0.3.111', '--signature', sig, '--asset', 'neati-macos-arm64.app.tar.gz', '--output', output, '--pub-date', '2026-11-22T00:00:00Z']);
    const manifest = JSON.parse(readFileSync(output, 'utf8'));
    expect(manifest.version).toBe('0.3.111');
    expect(manifest.pub_date).toBe('2026-11-22T00:00:00Z');
    expect(Object.keys(manifest.platforms)).toEqual(['darwin-aarch64']);
    expect(manifest.platforms['darwin-aarch64'].url).toBe(
      'https://github.com/jaeyoung0509/neati/releases/download/v0.3.111/neati-macos-arm64.app.tar.gz',
    );
    expect(manifest.platforms['darwin-aarch64'].signature).toContain('version:0.3.111');
  });

  it.each([
    ['a non-semver version', ['--version', 'latest']],
    ['an asset path that could escape the release', ['--asset', '../evil.app.tar.gz']],
    ['an asset that is not an app archive', ['--asset', 'neati.dmg']],
  ])('refuses %s', (_label, override) => {
    const { sig, output } = fixture();
    const args = new Map([['--version', '0.3.111'], ['--signature', sig], ['--asset', 'neati-macos-arm64.app.tar.gz'], ['--output', output]]);
    args.set(override[0], override[1]);
    const result = spawnSync('node', [script, ...[...args].flat()]);
    expect(result.status).toBe(1);
  });

  it('refuses an empty signature', () => {
    const { sig, output } = fixture('  \n');
    const result = spawnSync('node', [script, '--version', '0.3.111', '--signature', sig, '--asset', 'neati-macos-arm64.app.tar.gz', '--output', output]);
    expect(result.status).toBe(1);
    expect(result.stderr.toString()).toContain('signature file is empty');
  });
});
