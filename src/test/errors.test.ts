import { readdirSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { describeError, describeErrorMessage } from '../lib/utils/errors';

describe('describeError', () => {
  it('keeps the text of whatever was thrown', () => {
    expect(describeError('offline')).toBe('offline');
    expect(describeError(new Error('offline'))).toBe('Error: offline');
    expect(describeError(42)).toBe('42');
  });

  it('uses the fallback only when nothing usable was thrown', () => {
    expect(describeError(null, 'Scan failed')).toBe('Scan failed');
    expect(describeError(undefined, 'Scan failed')).toBe('Scan failed');
    expect(describeError('', 'Scan failed')).toBe('Scan failed');
    expect(describeError('offline', 'Scan failed')).toBe('offline');
    expect(describeError(null)).toBe('');
  });

  it('prefers a bare message when one exists', () => {
    expect(describeErrorMessage(new Error('port busy'))).toBe('port busy');
    expect(describeErrorMessage({ message: 'refused' })).toBe('refused');
    expect(describeErrorMessage('plain')).toBe('plain');
    expect(describeErrorMessage({ message: '' })).toBe('[object Object]');
  });
});

describe('type hygiene (#432)', () => {
  it('has no `any` in product source', () => {
    const srcRoot = fileURLToPath(new URL('../', import.meta.url));
    const files = readdirSync(srcRoot, { recursive: true })
      .filter((path): path is string => typeof path === 'string')
      .filter((path) => /\.(ts|svelte)$/.test(path))
      .filter((path) => !path.startsWith('test/') && !path.startsWith('lib/bindings/'));
    const offenders = files.filter((path) =>
      /(?::\s*any\b|\bas any\b|<any>)/.test(readFileSync(`${srcRoot}/${path}`, 'utf8'))
    );
    expect(offenders).toEqual([]);
  });
});
