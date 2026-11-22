import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { describe, expect, it } from 'vitest';

// Every cleanup workflow permanently removes what it authorizes (#416), so
// static interface copy may not promise a recoverable move to Trash (#475).
// Disposition-driven copy (`trashLabel` templates) is unaffected.
const root = join(__dirname, '..');
const scanned = ['routes', 'lib/components', 'lib/utils', 'lib/stores', 'lib/api/mocks'];

function files(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return files(path);
    return /\.(svelte|ts)$/.test(name) && !/\.test\.ts$/.test(name) ? [path] : [];
  });
}

describe('permanent-removal copy', () => {
  it('never promises that cleaned items move to Trash', () => {
    const claim = /\bmoves? to Trash\b|\bempty it to free\b|\bEmpty Trash to free\b/;
    const offending = scanned
      .flatMap((dir) => files(join(root, dir)))
      .filter((path) => claim.test(readFileSync(path, 'utf8')))
      .map((path) => relative(root, path));
    expect(offending).toEqual([]);
  });
});
