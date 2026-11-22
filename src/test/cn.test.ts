import { describe, expect, it } from 'vitest';
import { cn } from '../lib/utils/cn';

describe('cn', () => {
  it('keeps a named type step next to a text colour', () => {
    expect(cn('text-meta', 'text-muted-foreground')).toBe('text-meta text-muted-foreground');
  });
  it('lets a later type step replace an earlier one', () => {
    expect(cn('text-body font-medium', 'text-meta')).toBe('font-medium text-meta');
  });
  it('lets a caller override padding and width', () => {
    expect(cn('max-w-lg p-5', 'max-w-xl p-0')).toBe('max-w-xl p-0');
  });
  it('drops falsy conditional classes', () => {
    expect(cn('a', false && 'b', undefined, { c: true, d: false })).toBe('a c');
  });
});
