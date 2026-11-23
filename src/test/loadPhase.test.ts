import { afterEach, describe, expect, it, vi } from 'vitest';
import { render } from 'svelte/server';
import ApplicationsView from '../routes/dashboard/ApplicationsView.svelte';
import LargeFilesView from '../routes/dashboard/LargeFilesView.svelte';
import UsagePanel from '../lib/components/ai-activity/UsagePanel.svelte';
import { platformCapabilitiesStore } from '../lib/stores/platformCapabilities.svelte';
import { usageStore } from '../lib/stores/usage.svelte';
import { mockApi } from '../lib/api/mock';
import { awaitingFirstAnswer, loadFailed, loadStarted } from '../lib/utils/loadPhase';

// An empty result reads as final only after the first answer (#491).
describe('load phases', () => {
  afterEach(() => {
    platformCapabilitiesStore.reset();
    usageStore.loadPhase = 'idle';
    usageStore.isLoading = false;
    usageStore.snapshot = null;
    usageStore.error = null;
  });

  it('keeps an answer through later reads and failures', () => {
    expect(awaitingFirstAnswer('idle')).toBe(true);
    expect(awaitingFirstAnswer('loading')).toBe(true);
    expect(awaitingFirstAnswer('ready')).toBe(false);
    expect(awaitingFirstAnswer('error')).toBe(false);
    expect(loadStarted('idle')).toBe('loading');
    expect(loadStarted('error')).toBe('loading');
    expect(loadStarted('ready')).toBe('ready');
    expect(loadFailed('loading')).toBe('error');
    expect(loadFailed('ready')).toBe('ready');
  });

  it('shows Apps as loading until capabilities and the inventory answer', async () => {
    const pending = render(ApplicationsView, { props: { onBack: vi.fn() } }).body;
    expect(pending).toContain('Loading applications…');
    expect(pending).not.toContain('No applications found.');

    platformCapabilitiesStore.capabilities = await mockApi.getPlatformCapabilities();
    const reading = render(ApplicationsView, { props: { onBack: vi.fn() } }).body;
    expect(reading).toContain('Loading applications…');

    const answered = render(ApplicationsView, { props: { onBack: vi.fn(), initialApps: [] } }).body;
    expect(answered).toContain('No applications found.');
    expect(answered).not.toContain('Loading applications…');
  });

  it('does not wait for an inventory the platform cannot read', async () => {
    platformCapabilitiesStore.capabilities = {
      ...await mockApi.getPlatformCapabilities(),
      installed_apps: { status: 'unavailable', reason: 'Inventory is not supported here.' },
    };
    const body = render(ApplicationsView, { props: { onBack: vi.fn() } }).body;
    expect(body).not.toContain('Loading applications…');
    expect(body).not.toContain('No applications found.');
    expect(body).toContain('Inventory is not supported here.');
  });

  it('shows AI accounts as checking until the first usage snapshot', () => {
    usageStore.loadPhase = 'idle';
    const pending = render(UsagePanel).body;
    expect(pending).toContain('Checking account usage…');
    expect(pending).not.toContain('No AI account usage metadata is available yet.');

    usageStore.loadPhase = 'ready';
    const answered = render(UsagePanel).body;
    expect(answered).toContain('No AI account usage metadata is available yet.');
    expect(answered).not.toContain('Checking account usage…');
  });

  it('does not claim a previous usage snapshot when the first read fails', () => {
    usageStore.loadPhase = 'error';
    usageStore.error = 'Could not load AI usage';
    const body = render(UsagePanel).body;
    expect(body).toContain('Usage could not be loaded.');
    expect(body).not.toContain('The last successful usage snapshot remains visible.');
    expect(body).not.toContain('Checking account usage…');
  });

  it('shows Large Files as not yet scanned before a scan starts', () => {
    const body = render(LargeFilesView, { props: { onBack: vi.fn() } }).body;
    expect(body).toContain('No large-file scan yet.');
    expect(body).not.toContain('No files matched this size threshold.');
  });
});
