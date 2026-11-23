import { describe, expect, it } from 'vitest';
import { isFilesystemRoot } from '../lib/utils/scanRoots';

describe('scan progress roots (#493)', () => {
  it('shows real paths and hides owner-provider steps', () => {
    expect(isFilesystemRoot('/Users/me/Library/Caches')).toBe(true);
    expect(isFilesystemRoot('C:\\Users\\me\\AppData')).toBe(true);
    expect(isFilesystemRoot('provider://homebrew_cleanup')).toBe(false);
  });
});
