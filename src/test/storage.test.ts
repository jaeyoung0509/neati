import { afterEach, describe, expect, it, vi, beforeEach } from 'vitest';
import { render } from 'svelte/server';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import StorageView from '../routes/dashboard/StorageView.svelte';
import CategoryCard from '../lib/components/CategoryCard.svelte';
import StorageSummary from '../lib/components/StorageSummary.svelte';
import CategoryDetailView from '../routes/dashboard/CategoryDetailView.svelte';
import ItemRow from '../lib/components/ItemRow.svelte';
import CleanResultModal from '../lib/components/CleanResultModal.svelte';
import { scanStore } from '../lib/stores/scan.svelte';
import { platformCapabilitiesStore } from '../lib/stores/platformCapabilities.svelte';
import { mockApi } from '../lib/api/mock';
import type { CategoryResult, CleanResult, ScanItem } from '../lib/models/types';

// Keep Svelte's server-rendering environment; use jsdom only as an HTML parser.
const { JSDOM } = createRequire(import.meta.url)('jsdom') as {
  JSDOM: new (html: string) => { window: { document: Document } };
};
function markupDocument(body: string): Document {
  return new JSDOM(body).window.document;
}

afterEach(() => {
  platformCapabilitiesStore.reset();
  scanStore.lastScan = null;
  scanStore.selectedMap = {};
  scanStore.isScanning = false;
  scanStore.isCleaning = false;
  scanStore.isRefreshingAfterClean = false;
  scanStore.lastScanTrigger = null;
  scanStore.scanId = null;
  scanStore.currentRoot = null;
  scanStore.foundItemCount = 0;
  scanStore.isCancelling = false;
  scanStore.error = null;
  scanStore.discovery = { status: 'exhausted' };
  scanStore.refusedItems = {};
  scanStore.refusalMessage = null;
});

/** The Stop control's own tag, so only its disabled state is asserted. */
function stopControl(body: string): string {
  return body.match(/<button[^>]*aria-label="Stop scan"[^>]*>/)?.[0] ?? '';
}

/** Every button element Svelte rendered, so one control's state can be read alone. */
function buttonTags(body: string): string[] {
  return body.match(/<button[^>]*>[\s\S]*?<\/button>/g) ?? [];
}

function scanItem(overrides: Partial<ScanItem> & { id: string }): ScanItem {
  return {
    signature_id: overrides.id,
    name: overrides.id,
    category: 'developer',
    risk: 'safe',
    path: `/tmp/${overrides.id}`,
    size: { logical: 1024, allocated: 1024 },
    file_count: 1,
    description: 'Fixture row',
    is_selected: false,
    last_modified: null,
    exists: true,
    quality: 'fresh',
    incomplete_reason: null,
    ...overrides,
  } as ScanItem;
}

/**
 * Publishes a fresh scan for one category, so the copy under test is about
 * current eligibility rather than the age of the measurement.
 */
function publishScan(items: ScanItem[], scanId: string): CategoryResult {
  const nowSeconds = Math.floor(Date.now() / 1000);
  const totalBytes = items.reduce(
    (sum, entry) => sum + (entry.size.allocated ?? entry.size.logical),
    0
  );
  const category: CategoryResult = {
    category: 'developer',
    display_name: 'Developer',
    items,
    total_bytes: totalBytes,
    safe_bytes: 0,
    rebuild_bytes: 0,
    manual_bytes: 0,
  };
  scanStore.lastScan = {
    scan_id: scanId,
    valid_for_seconds: 300,
    started_at: nowSeconds - 1,
    finished_at: nowSeconds,
    categories: [category],
    total_bytes: totalBytes,
    safe_bytes: 0,
    rebuild_bytes: 0,
    manual_bytes: 0,
    quality: 'fresh',
    incomplete_reasons: [],
  };
  scanStore.syncSelectionFromScan(scanStore.lastScan);
  scanStore.updateFreshness();
  return category;
}

describe('Storage scan summary', () => {
  it('renders an unavailable empty scan without invented zero totals or disabled cleanup chrome', () => {
    publishScan([], 'unknown-empty');
    scanStore.lastScan!.categories = [];
    scanStore.lastScan!.quality = 'unavailable';
    const body = render(StorageView, { props: { onSelectCategory: vi.fn() } }).body;
    expect(body).toContain('Cleanup estimate unavailable');
    expect(body).toContain('No verified items to show yet');
    expect(body).toContain('Unread locations remain unknown');
    expect(body).not.toContain('0 B');
    expect(body).not.toContain('Select all available cleanup items');
    expect(body).not.toContain('Cleanup selection and actions');
  });

  it('distinguishes a verified empty scan from an unread partial empty scan', () => {
    publishScan([], 'verified-empty');
    scanStore.lastScan!.categories = [];
    const verified = render(StorageView, { props: { onSelectCategory: vi.fn() } }).body;
    expect(verified).toContain('No cache items found in checked locations');
    expect(verified).toContain('0 B');
    scanStore.lastScan!.quality = 'partial';
    const partial = render(StorageView, { props: { onSelectCategory: vi.fn() } }).body;
    expect(partial).toContain('No verified items to show yet');
    expect(partial).not.toContain('0 B');
    expect(partial).not.toContain('No cache items found');
  });

  it('keeps advisory bytes and explains retained items without a disabled bulk row', () => {
    publishScan([scanItem({ id: 'retained-temp', risk: 'manual', disposition: { eligibility: 'advisory', reason: null, cleanable_bytes: 0 } })], 'advisory-temp');
    const body = render(StorageView, { props: { onSelectCategory: vi.fn() } }).body;
    expect(body).toContain('1 KB');
    expect(body).toContain('Open a category to inspect the reasons');
    expect(body).not.toContain('Select all available cleanup items');
    expect(body).not.toContain('Cleanup selection and actions');
  });

  it('keeps verified ready bytes in a mixed partial scan and labels their limited coverage', () => {
    publishScan([scanItem({ id: 'verified-temp', disposition: { eligibility: 'auto_cleanable', reason: null, cleanable_bytes: 1024 } })], 'partial-temp');
    scanStore.lastScan!.quality = 'partial';
    const body = render(StorageView, { props: { onSelectCategory: vi.fn() } }).body;
    expect(body).toContain('Ready in checked locations');
    expect(body).toContain('Unknown bytes excluded');
    expect(body).toContain('Partial scan');
    expect(body).toContain('1 KB');
    expect(body).toContain('Select all available cleanup items');
  });

  it('puts a failed empty scan retry instruction beside its reason', () => {
    scanStore.error = 'The scan request failed';
    const body = render(StorageView, { props: { onSelectCategory: vi.fn() } }).body;
    expect(body).toContain('The storage scan did not finish');
    expect(body).toContain('Try Scan Storage again');
    expect(body).not.toContain('No cache items found');
  });
  it('separates observed bytes from bytes authorized for cleanup, including uncertain overlaps', () => {
    publishScan([
      scanItem({ id: 'cleanable', size: { logical: 2048, allocated: 2048 }, disposition: { eligibility: 'auto_cleanable', reason: null, cleanable_bytes: 1024 } }),
      scanItem({ id: 'kept', risk: 'manual', size: { logical: 8192, allocated: 8192 }, disposition: { eligibility: 'advisory', reason: null, cleanable_bytes: 0 } }),
    ], 'summary-overlap');
    scanStore.lastScan!.ambiguous_overlap_bytes = 2048;

    const { body } = render(StorageSummary);
    expect(body).toContain('Ready to clean now');
    expect(body).toContain('1 KB');
    expect(body).toContain('8 KB–10 KB');
    expect(body).toContain('Overlapping observations');
    const page = render(StorageView, { props: { onSelectCategory: vi.fn() } }).body;
    expect(page).toContain('Includes items that must be kept.');
  });

  it('puts ready bytes first when a much larger cache requires app exit', () => {
    const category = publishScan([
      scanItem({ id: 'ready', disposition: { eligibility: 'auto_cleanable', reason: null, cleanable_bytes: 1024 } }),
      scanItem({ id: 'running', owner_running: true, size: { logical: 1073741824, allocated: 1073741824 }, disposition: { eligibility: 'reviewable', reason: null, cleanable_bytes: 1073741824 } }),
    ], 'mixed');
    const summary = render(StorageSummary).body;
    const card = render(CategoryCard, { props: { categoryResult: category } }).body;
    expect(summary).toMatch(/text-metric-lg[^>]*>\s*1 KB/);
    expect(summary).toContain('1 GB requires idle apps');
    expect(card).toContain('1 GB requires idle apps');
    expect(card).toContain('Ready now');
    expect(card).not.toContain('Can clean');
  });

  it('labels an expired scan as a previous estimate', () => {
    publishScan([scanItem({ id: 'expired' })], 'summary-expired');
    const freshness = vi.spyOn(scanStore, 'freshness', 'get').mockReturnValue('stale');
    const { body } = render(StorageSummary);
    freshness.mockRestore();
    expect(body).toContain('Last ready-to-clean estimate');
    expect(body).toContain('Scan again to verify these results.');
    expect(body).not.toContain('Available to clean');
  });

  it('keeps selection before the complete grouped diagnostic inventory', () => {
    publishScan([
      scanItem({ id: 'ready', disposition: { eligibility: 'auto_cleanable', reason: null, cleanable_bytes: 1024 } }),
      scanItem({ id: 'review', disposition: { eligibility: 'reviewable', reason: 'Reviewed owner action', cleanable_bytes: 1024 } }),
      scanItem({ id: 'kept', risk: 'manual', disposition: { eligibility: 'advisory', reason: 'Keep this state', cleanable_bytes: 0 } }),
      scanItem({ id: 'unknown', quality: 'unavailable', size: { logical: 0, allocated: 0 }, disposition: { eligibility: 'blocked', reason: 'Could not inspect', cleanable_bytes: 0 } }),
    ], 'grouped-details');
    const body = render(StorageView, { props: { onSelectCategory: vi.fn() } }).body;
    const document = markupDocument(body);
    const details = document.querySelector('[data-cleanup-item-details]')!;
    const selection = document.querySelector('.category-list')!;
    const toolbar = document.querySelector('[aria-label="Cleanup selection and actions"]')!;
    expect(selection.compareDocumentPosition(details) & 4).toBe(4);
    expect(details.compareDocumentPosition(toolbar) & 4).toBe(4);
    expect(details.hasAttribute('open')).toBe(false);
    expect(details.querySelector('summary')?.textContent).toBe('Item details (3)');
    expect(details.querySelectorAll('li')).toHaveLength(3);
    expect(details.textContent).toContain('Review and owner actions (1)');
    expect(details.textContent).toContain('Kept and unverified observations (2)');
    expect(details.textContent).toContain('Keep this state');
    expect(details.textContent).toContain('Could not inspect');
    expect(scanStore.selectedMap.kept).toBe(false);
    expect(scanStore.selectedMap.unknown).toBe(false);
  });

  it('shows an unestimated action count without expanding provider explanations', () => {
    publishScan([scanItem({ id: 'owner', risk: 'rebuild', cache_metadata: {
      provider: 'pnpm', management_mode: 'tool_managed', artifact_kind: 'package_store',
      consequence: 'Packages may be downloaded again.', size_semantics: 'informational', last_used_confidence: 'unknown',
    }, disposition: { eligibility: 'reviewable', reason: 'The tool decides what is unused', cleanable_bytes: null } })], 'unestimated-details');
    const document = markupDocument(render(StorageView, { props: { onSelectCategory: vi.fn() } }).body);
    const summary = document.querySelector('[aria-label="Storage scan summary"]')!;
    expect(summary.textContent).toContain('1 action · Not estimated');
    expect(summary.textContent).toContain('Review items');
    expect(summary.textContent).not.toContain('The tool decides');
    const details = document.querySelector('[data-storage-scan-details]')!;
    expect(details.hasAttribute('open')).toBe(false);
    expect(details.textContent).toContain('Not estimated does not mean zero bytes');
  });

  it('shows a scan prompt without implying that an idle empty scan is still running', () => {
    const { body } = render(StorageView, { props: { onSelectCategory: vi.fn() } });
    expect(body).toContain('Start with a storage scan');
    expect(body).not.toContain('Scanning known development caches...');
    expect(body).not.toContain('0 B');
  });
});

describe('StorageView CTA and responsive toolbar layout', () => {
  beforeEach(() => {
    scanStore.lastScan = null;
    scanStore.selectedMap = {};
    scanStore.isScanning = false;
    scanStore.isCleaning = false;
    scanStore.error = null;
  });

  it('renders one clean action for safe-only selections without duplicating bytes in CTA text', () => {
    const mockCategory: CategoryResult = {
      category: 'ai',
      display_name: 'AI Tools',
      items: [
        {
          id: 'item-1',
          signature_id: 'sig.ai.1',
          name: 'Claude Cache',
          category: 'ai',
          risk: 'safe',
          path: '/tmp/claude',
          size: { logical: 1024 * 1024 * 100, allocated: 1024 * 1024 * 100 },
          file_count: 50,
          description: 'Claude session caches',
          is_selected: true,
          last_modified: Date.now(),
          exists: true,
          quality: 'fresh',
          disposition: {
            eligibility: 'auto_cleanable',
            reason: null,
            cleanable_bytes: 1024 * 1024 * 100,
          },
        },
      ],
      total_bytes: 1024 * 1024 * 100,
      safe_bytes: 1024 * 1024 * 100,
      rebuild_bytes: 0,
      manual_bytes: 0,
    };

    const nowSeconds = Math.floor(Date.now() / 1000);
    scanStore.lastScan = {
      scan_id: 'scan-1',
      valid_for_seconds: 300,
      started_at: nowSeconds - 1,
      finished_at: nowSeconds,
      categories: [mockCategory],
      total_bytes: 1024 * 1024 * 100,
      safe_bytes: 1024 * 1024 * 100,
      rebuild_bytes: 0,
      manual_bytes: 0,
    };
    scanStore.selectedMap = { 'item-1': true };

    const rendered = render(StorageView, {
      props: {
        onSelectCategory: vi.fn(),
      },
    });

    expect(rendered.body).toContain('Clean selected');
    expect(rendered.body).not.toContain('Clean Safely');
    // Ensure the CTA button strictly renders clean text without appended byte label
    expect(rendered.body).toContain('<span>Clean selected</span>');
    expect(rendered.body).not.toContain('Clean selected 100 MB');
    expect(rendered.body).not.toContain(' Safe</span>');
    // Ensure responsive toolbar classes for 960x660 baseline
    expect(rendered.body).toContain('flex flex-col sm:flex-row sm:items-center justify-between gap-3');
    expect(rendered.body).toContain('aria-label="Cleanup selection and actions"');
    expect(rendered.body).toContain('aria-label="Open storage settings"');
  });

  it('renders the same clean action when rebuildable items are selected', () => {
    const mockCategory: CategoryResult = {
      category: 'developer',
      display_name: 'Developer Caches',
      items: [
        {
          id: 'item-rebuild',
          signature_id: 'sig.dev.cargo',
          name: 'Cargo Target',
          category: 'developer',
          risk: 'rebuild',
          path: '/tmp/target',
          size: { logical: 1024 * 1024 * 500, allocated: 1024 * 1024 * 500 },
          file_count: 120,
          description: 'Rebuildable build artifacts',
          is_selected: true,
          last_modified: Date.now(),
          exists: true,
          quality: 'fresh',
          disposition: {
            eligibility: 'reviewable',
            reason: null,
            cleanable_bytes: 1024 * 1024 * 500,
          },
        },
      ],
      total_bytes: 1024 * 1024 * 500,
      safe_bytes: 0,
      rebuild_bytes: 1024 * 1024 * 500,
      manual_bytes: 0,
    };

    const nowSeconds = Math.floor(Date.now() / 1000);
    scanStore.lastScan = {
      scan_id: 'scan-2',
      valid_for_seconds: 300,
      started_at: nowSeconds - 1,
      finished_at: nowSeconds,
      categories: [mockCategory],
      total_bytes: 1024 * 1024 * 500,
      safe_bytes: 0,
      rebuild_bytes: 1024 * 1024 * 500,
      manual_bytes: 0,
    };
    scanStore.selectedMap = { 'item-rebuild': true };
    expect(scanStore.rebuildSelectedBytes).toBe(1024 * 1024 * 500);

    const rendered = render(StorageView, {
      props: {
        onSelectCategory: vi.fn(),
      },
    });

    expect(rendered.body).toContain('Clean selected');
    // Ensure the CTA button strictly renders clean text without appended byte label
    expect(rendered.body).toContain('<span>Clean selected</span>');
    expect(rendered.body).not.toMatch(/<button[^>]*>[^<]*Rebuildable/);
  });

  it('wires zero-byte manual selections to the shared toolbar contract', () => {
    const safeItem = {
      id: 'safe-item',
      signature_id: 'sig.safe',
      name: 'Safe Cache',
      category: 'developer' as const,
      risk: 'safe' as const,
      path: '/tmp/safe-cache',
      size: { logical: 1024, allocated: 1024 },
      file_count: 1,
      description: 'Safe cache',
      is_selected: true,
      last_modified: null,
      exists: true,
      quality: 'fresh' as const,
      disposition: {
        eligibility: 'auto_cleanable' as const,
        reason: null,
        cleanable_bytes: 1024,
      },
    };
    const manualItem = {
      ...safeItem,
      id: 'manual-item',
      signature_id: 'sig.manual',
      name: 'Manual Resource',
      path: '/tmp/manual-resource',
      risk: 'manual' as const,
      size: { logical: 0, allocated: 0 },
      disposition: {
        eligibility: 'blocked' as const,
        reason: 'Manual cleanup only',
        cleanable_bytes: null,
      },
    };
    const category: CategoryResult = {
      category: 'developer',
      display_name: 'Developer Caches',
      items: [safeItem, manualItem],
      total_bytes: 1024,
      safe_bytes: 1024,
      rebuild_bytes: 0,
      manual_bytes: 0,
    };
    const nowSeconds = Math.floor(Date.now() / 1000);
    scanStore.lastScan = {
      scan_id: 'scan-manual-zero',
      valid_for_seconds: 300,
      started_at: nowSeconds - 1,
      finished_at: nowSeconds,
      categories: [category],
      total_bytes: 1024,
      safe_bytes: 1024,
      rebuild_bytes: 0,
      manual_bytes: 0,
    };
    scanStore.selectedMap = { 'safe-item': true, 'manual-item': true };

    const rendered = render(StorageView, { props: { onSelectCategory: vi.fn() } });

    expect(rendered.body).toContain('1 item needs a separate action');
    expect(rendered.body).toContain('These items need a separate action');
  });

  it('keeps the review action enabled for a selected provider-backed manual item', () => {
    const providerItem = {
      id: 'windows.recycle_bin',
      signature_id: 'windows.recycle_bin',
      name: 'Recycle Bin',
      category: 'system' as const,
      risk: 'manual' as const,
      path: 'C:\\$Recycle.Bin',
      size: { logical: 4096, allocated: 4096 },
      file_count: 3,
      description: 'Windows Recycle Bin',
      cache_metadata: {
        provider: 'Windows',
        management_mode: 'tool_managed' as const,
        artifact_kind: 'temporary' as const,
        consequence: 'Items move to the Recycle Bin and can be restored until it is emptied.',
        size_semantics: 'physical_reclaimable' as const,
        last_used_confidence: 'unknown' as const,
      },
      unit: { kind: 'provider_action' as const, root: 'C:\\', path: 'C:\\$Recycle.Bin' },
      lifecycle_provider_action: true,
      requires_confirmation: true,
      disposition: { eligibility: 'reviewable' as const, reason: null, cleanable_bytes: 4096 },
      is_selected: false,
      last_modified: null,
      exists: true,
      quality: 'fresh' as const,
    };
    const category: CategoryResult = {
      category: 'system',
      display_name: 'System',
      items: [providerItem],
      total_bytes: 4096,
      safe_bytes: 0,
      rebuild_bytes: 0,
      manual_bytes: 4096,
    };
    const nowSeconds = Math.floor(Date.now() / 1000);
    scanStore.lastScan = {
      scan_id: 'scan-provider-action',
      valid_for_seconds: 300,
      started_at: nowSeconds - 1,
      finished_at: nowSeconds,
      categories: [category],
      total_bytes: 4096,
      safe_bytes: 0,
      rebuild_bytes: 0,
      manual_bytes: 4096,
      quality: 'fresh',
      incomplete_reasons: [],
    };
    scanStore.selectedMap = { 'windows.recycle_bin': true };
    scanStore.updateFreshness(); // The app's freshness tick, so the frozen clock matches the scan

    const rendered = render(StorageView, { props: { onSelectCategory: vi.fn() } });

    expect(rendered.body).not.toContain('These items need a separate action');
    const action = rendered.body
      .match(/<button[^>]*>[\s\S]*?<\/button>/g)
      ?.find(button => button.includes('Clean selected'));
    expect(action).toBeDefined();
    expect(action).not.toContain('disabled=""');
  });

  it('reads a stopped scan as a stop rather than a completed or failed one', () => {
    platformCapabilitiesStore.reset();
    scanStore.error = null;
    const nowSeconds = Math.floor(Date.now() / 1000);
    scanStore.lastScan = {
      scan_id: 'scan-cancelled',
      valid_for_seconds: 300,
      started_at: nowSeconds - 5,
      finished_at: nowSeconds,
      categories: [],
      total_bytes: 0,
      safe_bytes: 0,
      rebuild_bytes: 0,
      manual_bytes: 0,
      quality: 'partial',
      incomplete_reasons: ['Scan was cancelled before completion'],
      cancelled: true,
    };

    const rendered = render(StorageView, { props: { onSelectCategory: vi.fn() } });

    expect(rendered.body).toContain('Scan stopped before it finished');
    expect(rendered.body).toContain('locations it had not reached were not inspected');
    expect(rendered.body).toContain('Scan was cancelled before completion');
    expect(rendered.body).toContain('Nothing was removed, and scanning again is safe');
    // It must not claim the scan completed, and it must not read as a failure.
    expect(rendered.body).not.toContain('Partial scan completed');
    expect(rendered.body).not.toContain('bg-destructive/15');
  });

  it('reads a stopped scan as a stop in the category detail notice too', () => {
    const nowSeconds = Math.floor(Date.now() / 1000);
    const category: CategoryResult = {
      category: 'developer',
      display_name: 'Developer',
      items: [],
      total_bytes: 0,
      safe_bytes: 0,
      rebuild_bytes: 0,
      manual_bytes: 0,
    };
    const stopped = {
      scan_id: 'scan-cancelled-detail',
      valid_for_seconds: 300,
      started_at: nowSeconds - 5,
      finished_at: nowSeconds,
      categories: [category],
      total_bytes: 0,
      safe_bytes: 0,
      rebuild_bytes: 0,
      manual_bytes: 0,
      quality: 'partial' as const,
      incomplete_reasons: ['Scan was cancelled before completion'],
      cancelled: true,
    };
    scanStore.lastScan = stopped;

    const rendered = render(CategoryDetailView, {
      props: { categoryResult: category, onBack: vi.fn(), onNavigateTab: vi.fn() },
    });

    expect(rendered.body).toContain('Scan stopped before it finished');
    expect(rendered.body).toContain('Nothing was removed, and scanning again is safe');
    expect(rendered.body).not.toContain('Partial scan completed');

    // A partial scan that ran to the end keeps its own copy.
    scanStore.lastScan = { ...stopped, cancelled: false };
    const partial = render(CategoryDetailView, {
      props: { categoryResult: category, onBack: vi.fn(), onNavigateTab: vi.fn() },
    });
    expect(partial.body).toContain('Some locations could not be checked');
    expect(partial.body).not.toContain('Scan stopped before it finished');
  });

  it('shows the root it is reading, what it has found, and a Stop control while scanning', () => {
    scanStore.isScanning = true;
    scanStore.currentRoot = { name: 'Cursor Editor Cache', path: '/Users/dev/Library/Caches/Cursor' };
    scanStore.foundItemCount = 3;

    const rendered = render(StorageView, { props: { onSelectCategory: vi.fn() } });
    expect(rendered.body).toContain('Checking storage');
    expect(rendered.body).toContain('Cursor Editor Cache');
    expect(rendered.body).toContain('/Users/dev/Library/Caches/Cursor');
    expect(rendered.body).toContain('Show current path');
    expect(rendered.body).toContain('3 items found so far');
    expect(stopControl(rendered.body)).not.toContain('disabled=""');

    // A stop already requested cannot be sent twice.
    scanStore.isCancelling = true;
    const stopping = render(StorageView, { props: { onSelectCategory: vi.fn() } });
    expect(stopping.body).toContain('Stopping…');
    expect(stopping.body).toContain('aria-label="Stopping scan"');
    expect(stopping.body).toContain('disabled=""');
  });

  it('keeps stale categories and review controls out of the post-cleanup verification phase', () => {
    publishScan([scanItem({ id: 'obsolete-cache', name: 'Old cache' })], 'old-inventory');
    scanStore.isScanning = true;
    scanStore.isRefreshingAfterClean = true;
    scanStore.currentRoot = { name: 'Tool caches', path: '/Users/dev/Library/Caches/Tool' };

    const body = render(StorageView, { props: { onSelectCategory: vi.fn() } }).body;
    expect(body).toContain('Checking storage after cleanup');
    expect(body).toContain('Cleanup finished. A new scan is checking what remains');
    expect(body).toContain('Stop scan');
    expect(body).not.toContain('Old cache');
    expect(body).not.toContain('Clean selected');
    expect(body).not.toContain('Last ready-to-clean estimate');
  });

  it('uses the header scan control as the only freshness status UI', () => {
    scanStore.isScanning = true;
    scanStore.lastScanTrigger = 'auto';

    const rendered = render(StorageView, { props: { onSelectCategory: vi.fn() } });

    expect(rendered.body).toContain('animate-bounce-dot-1');
    expect(rendered.body).toContain('Scanning…');
    expect(rendered.body).not.toContain('Scan storage to find current cleanup candidates');
    expect(rendered.body).not.toContain('Scan Again');
  });

  it('renders the header scan control with a stable square wrapper and paint-only motion', () => {
    scanStore.isScanning = false;
    const idleRender = render(StorageView, { props: { onSelectCategory: vi.fn() } });
    expect(idleRender.body).toContain('id="storage-scan-button"');
    expect(idleRender.body).toContain('inline-flex items-center justify-center shrink-0 w-3.5 h-3.5');
    expect(idleRender.body).toContain('transition-[background-color,color,border-color]');

    scanStore.isScanning = true;
    const scanningRender = render(StorageView, { props: { onSelectCategory: vi.fn() } });
    expect(scanningRender.body).toContain('id="storage-scan-button"');
    expect(scanningRender.body).toContain('inline-flex items-center justify-center shrink-0 w-3.5 h-3.5');
    expect(scanningRender.body).toContain('animate-bounce-dot-1');
  });

  it('labels the native workflow selector and keeps its region programmatically focusable', () => {
    const rendered = render(StorageView, { props: { onSelectCategory: vi.fn() } });
    expect(rendered.body).toMatch(/<select[^>]*aria-label="Storage workflows"[^>]*aria-controls=/);
    expect(rendered.body).not.toContain('role="tablist"');
    expect(rendered.body).toContain('Storage tools');
    expect(rendered.body).toContain('Developer Artifacts');
    expect(rendered.body).toContain('Large Files');
    expect(rendered.body).toContain('Disks');
    expect(rendered.body).toContain('tabindex="-1"');
    expect(rendered.body).toContain('outline-none');
    expect(rendered.body).toMatch(/class="space-y-4 rounded-xl outline-none"[^>]*role="region"[^>]*tabindex="-1"/);
  });

  it('renders CleanResultModal with accessible dialog semantics and deterministic done button', () => {
    const mockResult: CleanResult = {
      plan_id: 'plan-done',
      started_at: 1000,
      finished_at: 1005,
      total_reclaimed_bytes: 1024 * 1024 * 50,
      total_failed_bytes: 0,
      partial_count: 0,
      failed_count: 0,
      actual_disk_free_delta: 1024 * 1024 * 50,
      items: [
        {
          item_id: 'item-1',
          name: 'Xcode DerivedData',
          path: '/Users/test/Library/Developer/Xcode/DerivedData',
          bytes_reclaimed: 1024 * 1024 * 50,
          success: true,
          status: 'success',
          failure_reason: null,
          error_message: null,
        },
      ],
    };

    const rendered = render(CleanResultModal, {
      props: { result: mockResult, onClose: vi.fn() },
    });

    expect(rendered.body).toContain('<dialog');
    expect(rendered.body).toContain('aria-modal="true"');
    expect(rendered.body).toContain('aria-labelledby=');
    expect(rendered.body).toContain('aria-describedby=');
    expect(rendered.body).toContain('Cleanup complete');
    expect(rendered.body).toContain('50 MB');
    expect(rendered.body).toContain('-done-button');
  });

  it('keeps Storage scan feedback off transform and opacity compositor animations', () => {
    const css = readFileSync(new URL('../app.css', import.meta.url), 'utf-8');
    const keyframesIndex = css.indexOf('@keyframes loading-dash');
    expect(keyframesIndex).toBeGreaterThan(0);
    const keyframesBlock = css.slice(keyframesIndex, css.indexOf('.animate-loading-dash'));
    expect(keyframesBlock).toContain('stroke-dashoffset:');
    expect(keyframesBlock).not.toContain('transform:');
    expect(keyframesBlock).not.toContain('rotate:');
    expect(keyframesBlock).not.toContain('opacity:');

    expect(css).toContain('@media (prefers-reduced-motion: reduce)');
    const reducedMotionIndex = css.indexOf('@media (prefers-reduced-motion: reduce)');
    const reducedMotionBlock = css.slice(reducedMotionIndex);
    expect(reducedMotionBlock).toContain('.animate-loading-dash');
    expect(reducedMotionBlock).toContain('animation: none !important');

    const storageView = readFileSync(
      new URL('../routes/dashboard/StorageView.svelte', import.meta.url),
      'utf-8'
    );
    const storageTools = readFileSync(
      new URL('../routes/dashboard/StorageTools.svelte', import.meta.url),
      'utf-8'
    );
    const quickPanel = readFileSync(
      new URL('../routes/quick/QuickPanel.svelte', import.meta.url),
      'utf-8'
    );

    expect(storageView).toContain('motion="paint"');
    expect(storageTools).toContain('motion="paint"');
    expect(storageView).toContain('<DeletingDots');
    expect(storageTools).toContain('<LoadingSpinner');
    expect(quickPanel).toContain('<DeletingDots');
    expect(storageView).not.toContain('animate-gentle-spin');
    expect(storageTools).not.toContain('animate-gentle-spin');
  });

  it('reveals scanned large files through the backend-owned inventory', () => {
    const largeFilesView = readFileSync(
      new URL('../routes/dashboard/LargeFilesView.svelte', import.meta.url),
      'utf-8'
    );

    // The backend resolves the path from its own inventory; rebuilding it in
    // the view would mix separators on Windows (`display_parent` + '/' + name).
    expect(largeFilesView).toContain('tauriRevealLargeFile(item.id)');
    expect(largeFilesView).not.toContain('display_parent}/${item.name}');
    expect(largeFilesView).not.toContain('tauriShowInFileManager(`${item.display_parent}');
  });
});

describe('detected versus reclaimable storage copy', () => {
  it('keeps container storage unselected and explains its dedicated action', () => {
    const bytes = 6 * 1024 * 1024 * 1024;
    const category: CategoryResult = {
      category: 'container',
      display_name: 'Docker & Containers',
      items: [
        {
          id: 'container.orbstack.storage',
          signature_id: 'adapter.orbstack.storage',
          name: 'OrbStack VM Storage',
          category: 'container',
          risk: 'manual',
          path: '/Users/test/Library/Group Containers/HUAQ24HBR6.dev.orbstack/data/data.img.raw',
          size: { logical: 240 * 1024 * 1024 * 1024, allocated: bytes },
          file_count: 1,
          description: 'Active container and Linux VM data.',
          is_selected: false,
          last_modified: null,
          exists: true,
          quality: 'fresh',
          disposition: {
            eligibility: 'blocked',
            reason: 'Manual cleanup only',
            cleanable_bytes: null,
          },
        },
      ],
      total_bytes: bytes,
      safe_bytes: 0,
      rebuild_bytes: 0,
      manual_bytes: bytes,
    };

    const nowSeconds = Math.floor(Date.now() / 1000);
    scanStore.lastScan = {
      scan_id: 'scan-orbstack',
      valid_for_seconds: 300,
      started_at: nowSeconds - 1,
      finished_at: nowSeconds,
      categories: [category],
      total_bytes: bytes,
      safe_bytes: 0,
      rebuild_bytes: 0,
      manual_bytes: bytes,
    };
    scanStore.syncSelectionFromScan(scanStore.lastScan);

    const rendered = render(CategoryDetailView, {
      props: { categoryResult: category, onBack: vi.fn(), onNavigateTab: vi.fn() },
    });

    expect(rendered.body).toContain('1 item');
    expect(rendered.body).toContain('Manage containers');
    expect(rendered.body).not.toContain('reclaimable locations');
    expect(rendered.body).toContain('neati reports their storage without deleting it');
    expect(scanStore.selectedMap['container.orbstack.storage']).toBe(false);
    expect(scanStore.reclaimableBytes).toBe(0);
  });

  it('hides the Applications tab when installed_apps is unavailable', async () => {
    platformCapabilitiesStore.capabilities = {
      ...(await mockApi.getPlatformCapabilities()),
      platform: 'windows',
      installed_apps: {
        status: 'unavailable',
        reason: 'Windows application inventory is not supported. Use Windows Settings.',
      },
    };

    const rendered = render(StorageView, {
      props: { onSelectCategory: vi.fn() },
    });

    expect(rendered.body).not.toContain('Applications');
    expect(rendered.body).toContain('Cleanup');
    expect(rendered.body).toContain('Large Files');
    expect(rendered.body).toContain('Developer Artifacts');
    expect(rendered.body).toContain('Disks');
  });

  it('shows the Applications tab when installed_apps is available', async () => {
    platformCapabilitiesStore.capabilities = {
      ...(await mockApi.getPlatformCapabilities()),
      platform: 'macos',
      installed_apps: { status: 'available' },
    };

    const rendered = render(StorageView, {
      props: { onSelectCategory: vi.fn() },
    });

    expect(rendered.body).toContain('Applications');
  });

  it('shows the Applications tab when installed_apps is read_only', async () => {
    platformCapabilitiesStore.capabilities = {
      ...(await mockApi.getPlatformCapabilities()),
      platform: 'windows',
      installed_apps: { status: 'read_only', reason: 'Inspection only' },
    };

    const rendered = render(StorageView, {
      props: { onSelectCategory: vi.fn() },
    });

    expect(rendered.body).toContain('Applications');
  });
});

describe('StorageView scan remediation', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-01-01T00:00:00Z'));
    scanStore.updateFreshness();
    // Reproduce a scan completing after the store's last clock observation.
    vi.advanceTimersByTime(2_000);
  });

  afterEach(() => {
    scanStore.lastScan = null;
    vi.useRealTimers();
    scanStore.updateFreshness();
  });

  it.each([
    ['tool_missing', 'Required tool is unavailable'],
    ['unsupported_adapter', 'unsupported by the current adapter'],
    ['unknown', 'cause is unknown'],
    ['permission_denied', 'check file ownership and permissions'],
  ] as const)('shows the typed %s cause without privacy guidance', (kind, label) => {
    const now = Math.floor(Date.now() / 1000);
    scanStore.lastScan = { scan_id: 'typed-gap', valid_for_seconds: 300,
      started_at: now - 1, finished_at: now, categories: [],
      total_bytes: 0, safe_bytes: 0, rebuild_bytes: 0, manual_bytes: 0,
      quality: 'unavailable', gaps: [{ kind, count: 2 }],
      incomplete_reasons: ['Operation not permitted; Full Disk Access'],
    };
    scanStore.updateFreshness();
    const body = render(StorageView, { props: { onSelectCategory: vi.fn() } }).body;
    expect(body).toContain(label);
    expect(body).not.toContain('Open System Settings');
    expect(body).not.toContain('Check Access');
  });

  it('surfaces Full Disk Access guidance on the main cleanup view', () => {
    const nowSeconds = Math.floor(Date.now() / 1000);
    scanStore.lastScan = {
      scan_id: 'scan-fda',
      valid_for_seconds: 300,
      started_at: nowSeconds - 1,
      finished_at: nowSeconds,
      categories: [],
      total_bytes: 0,
      safe_bytes: 0,
      rebuild_bytes: 0,
      manual_bytes: 0,
      quality: 'unavailable',
      incomplete_reasons: ['Operation not permitted'],
      gaps: [{ kind: 'full_disk_access', count: 248 }],
      cancelled: false,
    };
    scanStore.updateFreshness();
    expect(scanStore.freshness).toBe('unavailable');

    const rendered = render(StorageView, {
      props: {
        onSelectCategory: vi.fn(),
      },
    });

    const text = rendered.body.replace(/<[^>]*>/g, '').replace(/\s+/g, ' ');
    expect(text).toContain('248 checks');
    expect(text).toContain('select the neati.app you use');
    expect(text).toContain('does not grant administrator access');
    expect(text).toContain('Check Access');
    expect(text).not.toContain('Access granted');
    expect(rendered.body).toContain('Open System Settings');
    expect(rendered.body).not.toContain('Partial scan completed');
    const document = markupDocument(rendered.body);
    const accessHelp = document.querySelector('[data-storage-access-setup] details')!;
    expect(accessHelp.hasAttribute('open')).toBe(false);
    expect(accessHelp.querySelector('summary')?.textContent).toBe('Manage access');
    expect(accessHelp.textContent).toContain('does not grant administrator access');
    const reasons = document.querySelector('[data-storage-scan-details]')!;
    expect(reasons.hasAttribute('open')).toBe(false);
    expect(reasons.textContent).toContain('file permissions can also deny access');
  });
});

describe('risk classification versus current eligibility', () => {
  function renderCategory(category: CategoryResult) {
    return render(CategoryDetailView, {
      props: { categoryResult: category, onBack: vi.fn(), onNavigateTab: vi.fn() },
    });
  }

  it('keeps a selected owner prune usable when its reclaim amount is unknown', () => {
    const category = publishScan(
      [
        scanItem({
          id: 'owner-store',
          risk: 'rebuild',
          cache_metadata: {
            provider: 'pnpm',
            management_mode: 'tool_managed',
            artifact_kind: 'package_store',
            consequence: 'Unused packages may be downloaded again.',
            size_semantics: 'informational',
            last_used_confidence: 'unknown',
          },
          disposition: {
            eligibility: 'reviewable',
            reason: 'The package manager decides what can be pruned',
            cleanable_bytes: null,
          },
        }),
      ],
      'scan-owner-store'
    );
    scanStore.setItemSelected('owner-store', true);

    const detail = renderCategory(category).body;
    expect(detail).toContain('Reclaim size not estimated');
    const ownerButton = buttonTags(detail).find(button => button.includes('Review owner cleanup')) ?? '';
    expect(ownerButton).not.toMatch(/\sdisabled(?:\s|=|>)/);

    const overview = render(StorageView, { props: { onSelectCategory: vi.fn() } }).body;
    expect(overview).toContain('Reclaim size not estimated');
    const cleanButton = buttonTags(overview).find(button => button.includes('Clean selected')) ?? '';
    expect(cleanButton).not.toMatch(/\sdisabled(?:\s|=|>)/);
    expect(overview).toContain('1 tool-managed cleanup has');
    expect(detail).toContain('pnpm decides what is unused');

    publishScan([...category.items, scanItem({
      id: 'known-cache',
      disposition: { eligibility: 'auto_cleanable', reason: null, cleanable_bytes: 1024 },
    })], 'scan-mixed-estimates');
    scanStore.setItemSelected('owner-store', true);
    scanStore.setItemSelected('known-cache', true);
    const mixed = render(StorageView, { props: { onSelectCategory: vi.fn() } }).body;
    expect(mixed).toContain('1 without a size estimate');
  });

  it('shows a tab of recent and advisory rows as zero selectable inventory, not as broken', () => {
    const category = publishScan(
      [
        scanItem({
          id: 'recent-cache',
          name: 'Recent cache',
          disposition: {
            eligibility: 'recent',
            reason: 'Last used 2 days ago; the age policy needs 7',
            cleanable_bytes: null,
          },
        }),
        scanItem({
          id: 'advisory-cache',
          name: 'Advisory cache',
          disposition: {
            eligibility: 'advisory',
            reason: 'Managed by its own tool',
            cleanable_bytes: null,
          },
        }),
      ],
      'scan-eligibility'
    );

    const { body } = renderCategory(category);

    expect(body).toContain('2 items · Nothing ready yet');
    expect(body).toContain('Nothing ready yet');
    const selectFiltered = buttonTags(body).find((button) => button.includes('Select all')) ?? '';
    expect(selectFiltered).toContain('disabled');
    expect(body).toContain('title="Nothing ready yet"');
    expect(body).not.toContain('Filter cleanup items by risk');
    expect(body).toContain('aria-label="Filter cleanup items by name or path"');
    // Inventory that is not cleanable yet is not a failure.
    expect(body).not.toContain('Scan again');
    expect(body).not.toContain('bg-destructive/15');
    expect(body).toContain('Recent cache');
    expect(body).toContain('Advisory cache');
  });

  it('states the currently cleanable count and bytes beside the detected ones only when they differ', () => {
    const mixed = publishScan(
      [
        scanItem({
          id: 'cleanable-cache',
          size: { logical: 4096, allocated: 4096 },
          disposition: { eligibility: 'auto_cleanable', reason: null, cleanable_bytes: 4096 },
        }),
        scanItem({
          id: 'recent-cache',
          size: { logical: 2048, allocated: 2048 },
          disposition: { eligibility: 'recent', reason: 'Too new', cleanable_bytes: null },
        }),
      ],
      'scan-mixed'
    );

    const mixedBody = renderCategory(mixed).body;
    expect(mixedBody).toContain('2 items · 4 KB can be cleaned');
    expect(mixedBody).not.toContain('Nothing ready yet');
    expect(
      buttonTags(mixedBody).find((button) => button.includes('Select all')) ?? ''
    ).not.toContain('disabled');

    // Every detected row is cleanable: there is nothing to qualify.
    const uniform = publishScan(
      [
        scanItem({
          id: 'cleanable-cache',
          size: { logical: 4096, allocated: 4096 },
          disposition: { eligibility: 'auto_cleanable', reason: null, cleanable_bytes: 4096 },
        }),
      ],
      'scan-uniform'
    );
    expect(renderCategory(uniform).body).toContain('1 item · 4 KB can be cleaned');
  });

  it('does not claim every visible Rebuild row is removable when some are not', () => {
    const category = publishScan(
      [
        scanItem({
          id: 'rebuild-removable',
          risk: 'rebuild',
          size: { logical: 2048, allocated: 2048 },
          disposition: { eligibility: 'reviewable', reason: null, cleanable_bytes: 2048 },
        }),
        scanItem({
          id: 'rebuild-recent',
          risk: 'rebuild',
          size: { logical: 1024, allocated: 1024 },
          disposition: { eligibility: 'recent', reason: 'Used yesterday', cleanable_bytes: null },
        }),
      ],
      'scan-rebuild-mixed'
    );

    const mixedBody = renderCategory(category).body;
    expect(mixedBody).toContain('2 items · 2 KB can be cleaned');
    expect(mixedBody).toContain('Used yesterday');
    expect(mixedBody).not.toContain('Rebuild rows');

    const allRemovable = publishScan(
      [
        scanItem({
          id: 'rebuild-removable',
          risk: 'rebuild',
          size: { logical: 2048, allocated: 2048 },
          disposition: { eligibility: 'reviewable', reason: null, cleanable_bytes: 2048 },
        }),
      ],
      'scan-rebuild-uniform'
    );
    expect(renderCategory(allRemovable).body).not.toContain('not removable right now');
  });
});

describe('item-scoped cleanup refusals', () => {
  it('renders the recorded refusal on its own row and leaves the row selectable', () => {
    const refused = scanItem({
      id: 'refused-cache',
      size: { logical: 4096, allocated: 4096 },
      disposition: { eligibility: 'auto_cleanable', reason: null, cleanable_bytes: 4096 },
    });
    const unrelated = scanItem({
      id: 'unrelated-cache',
      size: { logical: 4096, allocated: 4096 },
      disposition: { eligibility: 'auto_cleanable', reason: null, cleanable_bytes: 4096 },
    });
    publishScan([refused, unrelated], 'scan-refusal');
    scanStore.refusedItems = {
      'refused-cache': 'Refused: this row is in use by another process.',
    };

    const { body } = render(ItemRow, { props: { item: refused } });

    expect(body).toContain('Refused: this row is in use by another process.');
    expect(body).toContain('title="Refused: this row is in use by another process."');
    // The refusal is a reason, never a block: the row keeps its own selection.
    const checkbox = body.match(/<input[^>]*type="checkbox"[^>]*>/)?.[0] ?? '';
    expect(checkbox).not.toContain('disabled');

    // A row the refusal does not name stays unmarked.
    expect(render(ItemRow, { props: { item: unrelated } }).body).not.toContain(
      'in use by another process'
    );
  });
});
