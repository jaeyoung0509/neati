import { afterEach, describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import AppLeftoverReview from '../lib/components/AppLeftoverReview.svelte';
import { mockAppLeftoverInventory } from '../lib/api/mocks/appLeftovers';
import { mockStorageApi } from '../lib/api/storage';
import { platformCapabilitiesStore } from '../lib/stores/platformCapabilities.svelte';
import { platformContextStore } from '../lib/stores/platformContext.svelte';
import { goldenPlatformContextByPlatform } from '../lib/models/platformContext';

afterEach(() => {
  platformCapabilitiesStore.reset();
  platformContextStore.context = null;
});

describe('read-only application resource review', () => {
  it('keeps observation work closed until explicitly opened', () => {
    const { body } = render(AppLeftoverReview);
    expect(body).toContain('Review possible app leftovers');
    expect(body).toContain('Read-only');
    expect(body).not.toContain('Refresh review');
    expect(body).not.toContain('Read-only application resource inventory');
  });

  it('shows typed uncertainty, protected state and Reveal without a removal action', () => {
    platformContextStore.context = goldenPlatformContextByPlatform.macos;
    const { body } = render(AppLeftoverReview, {
      props: { initialOpen: true, initialInventory: mockAppLeftoverInventory },
    });
    expect(body).toContain('Possible removed owner');
    expect(body).toContain('Owner uncertain or shared');
    expect(body).toContain('Protected state');
    expect(body).toContain('separate from Cleanup estimates');
    expect(body).toContain(goldenPlatformContextByPlatform.macos.reveal_label);
    expect(body).not.toMatch(/<button[^>]*>[^<]*(?:Clean|Delete|Remove)/);
    // The single checkbox is an installed-owner display filter, never a
    // per-resource selection or authorization control.
    expect(body.match(/type="checkbox"/g)).toHaveLength(1);
    expect(body).toContain('Include resources with an installed owner');
    expect(body).not.toContain('com.microsoft.VSCode');
  });

  it('does not turn a failed inventory or unknown measurement into a zero/empty result', () => {
    const inventory = structuredClone(mockAppLeftoverInventory);
    inventory.quality = 'partial';
    inventory.incomplete_reasons = ['Application root access denied'];
    inventory.items[0].quality = 'unavailable';
    inventory.items[0].allocated_size = 0;
    inventory.items[1].quality = 'partial';
    const { body } = render(AppLeftoverReview, { props: { initialOpen: true, initialInventory: inventory } });
    expect(body).toContain('Partial resource review');
    expect(body).toContain('Application root access denied');
    expect(body).toContain('Unknown size');
    expect(body).toContain('At least');
    inventory.items = [];
    const empty = render(AppLeftoverReview, { props: { initialOpen: true, initialInventory: inventory } }).body;
    expect(empty).toContain('Unchecked locations and owners remain unknown');
    expect(empty).not.toContain('No possible leftovers were identified');
  });

  it('returns a domain preview fixture without sharing mutable inventory state', async () => {
    const first = await mockStorageApi.getAppLeftovers();
    expect(Object.keys(first).sort()).toEqual(['incomplete_reasons', 'items', 'limitation', 'observed_roots', 'quality', 'skipped_entry_count']);
    first.items[0].classification = 'installed_owner';
    expect((await mockStorageApi.getAppLeftovers()).items[0].classification).toBe('possible_removed_owner');
  });
});
