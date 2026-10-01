import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import DeveloperArtifactsView from '../routes/dashboard/DeveloperArtifactsView.svelte';
import { mockStorageApi } from '../lib/api/storage';
import { artifactPathsOverlap, nonOverlappingArtifactIds, remainingArtifactScopes, toggleArtifactSelection } from '../lib/utils/developerArtifactSelection';
import { frameworkArtifactFixtures } from '../lib/api/mocks/frameworkArtifacts';
import { setPreviewPlatform } from '../lib/api/mocks/previewPlatform';

describe('developer artifact review workflow', () => {
  it('keeps framework mutation unavailable in Windows and Linux preview fixtures', async () => {
    try {
      for (const platform of ['windows', 'linux'] as const) {
        setPreviewPlatform(platform);
        const result = await mockStorageApi.startDeveloperArtifactScan(['workspace-myproject'], () => undefined);
        const whole = result.items.find(item => item.id === 'whole-next_output')!;
        const child = result.items.find(item => item.id === 'whole-child-next_output')!;
        expect(whole.status).toBe('observation_only');
        expect(child.status).toBe('safety_blocked');
        await expect(mockStorageApi.prepareDeveloperArtifactCleanup(result.scan_id, [whole.id])).rejects.toThrow('unavailable');
        await expect(mockStorageApi.prepareDeveloperArtifactCleanup(result.scan_id, [child.id])).rejects.toThrow('safety checks');
      }
    } finally { setPreviewPlatform('macos'); }
  });
  it('exposes whole default and custom observed scopes with exact manual plan accounting', async () => {
    const result = await mockStorageApi.startDeveloperArtifactScan(['workspace-myproject'], () => undefined);
    const whole = result.items.find(item => item.id === 'whole-next_output')!;
    const child = result.items.find(item => item.id === 'whole-child-next_output')!;
    const custom = result.items.find(item => item.id === 'custom-next-output')!;
    expect(whole.selected_by_default).toBe(false);
    expect(whole.status).toBe('complete');
    await expect(mockStorageApi.prepareDeveloperArtifactCleanup(result.scan_id, [whole.id])).resolves.toMatchObject({ allocated_size: whole.allocated_bytes, item_count: 1 });
    await expect(mockStorageApi.prepareDeveloperArtifactCleanup(result.scan_id, [whole.id, child.id])).rejects.toThrow('overlap');
    await expect(mockStorageApi.prepareDeveloperArtifactCleanup(result.scan_id, [custom.id])).rejects.toThrow('unavailable');
    const body = render(DeveloperArtifactsView, { props: { onBack: () => undefined, initialResult: { ...result, items: [whole, child, custom] } } }).body;
    expect(body).toContain('generated/site');
    expect(body).toContain('Observed output · no cleanup scope');
    expect(body).toContain('whole folder or generated child');
    expect(body).not.toContain('checked');
  });

  it('replaces a selected parent with its child and keeps unrelated sibling names', () => {
    const [parent, child] = frameworkArtifactFixtures;
    const sibling = { ...child, id: 'sibling', path: `${parent.path}-authored` };
    expect(toggleArtifactSelection([parent.id, sibling.id], child, [parent, child, sibling])).toEqual([sibling.id, child.id]);
    expect(toggleArtifactSelection([child.id, sibling.id], parent, [parent, child, sibling])).toEqual([sibling.id, parent.id]);
    expect(new Set(nonOverlappingArtifactIds([child, parent, sibling]))).toEqual(new Set([parent.id, sibling.id]));
    expect(artifactPathsOverlap('C:\\workspace\\.next', 'c:/WORKSPACE/.next/cache/webpack')).toBe(true);
    expect(artifactPathsOverlap('/fixture/.next', '/fixture/.next-authored')).toBe(false);
    expect(artifactPathsOverlap('/fixture/Case', '/fixture/case/child')).toBe(false);
    expect(remainingArtifactScopes([parent, child, sibling], new Set([child.id]))).toEqual([sibling]);
    expect(remainingArtifactScopes([parent, child, sibling], new Set([parent.id]))).toEqual([sibling]);
    expect(remainingArtifactScopes([parent, child, sibling], new Set())).toEqual([parent, child, sibling]);
  });
  it('streams marker-backed candidates with empty default selection', async () => {
    const events: string[] = [];
    const result = await mockStorageApi.startDeveloperArtifactScan(['workspace-myproject'], (event) => {
      events.push(event.type);
    });

    expect(events).toContain('artifact_found');
    expect(result.items.length).toBeGreaterThan(0);
    expect(result.items.every((item) => item.selected_by_default === false)).toBe(true);
    expect(result.items.some((item) => item.ecosystem === 'rust')).toBe(true);
  });

  it('allows measurement-incomplete candidates only through explicit selection', async () => {
    const result = await mockStorageApi.startDeveloperArtifactScan(
      ['workspace-myproject', 'workspace-work'],
      () => undefined
    );
    const incomplete = result.items.find((item) => item.status === 'measurement_incomplete');
    expect(incomplete).toBeDefined();
    await expect(
      mockStorageApi.prepareDeveloperArtifactCleanup(result.scan_id, [incomplete!.id])
    ).resolves.toMatchObject({ item_count: 1 });
  });

  it('keeps safety-blocked candidates out of cleanup plans', async () => {
    const result = await mockStorageApi.startDeveloperArtifactScan(['workspace-work'], () => undefined);
    const blocked = result.items.find((item) => item.status === 'safety_blocked');
    expect(blocked).toBeDefined();
    await expect(
      mockStorageApi.prepareDeveloperArtifactCleanup(result.scan_id, [blocked!.id])
    ).rejects.toThrow('safety checks');
  });

  it('keeps framework observations visible and unavailable for cleanup', async () => {
    const result = await mockStorageApi.startDeveloperArtifactScan(['workspace-work'], () => undefined);
    const observations = result.items.filter((item) => item.status === 'observation_only');
    expect(observations.map((item) => item.kind).sort()).toEqual(['next_output', 'svelte_kit_output']);
    expect(observations.every((item) => !item.selected_by_default && item.logical_bytes > 0)).toBe(true);
    for (const item of observations) {
      await expect(mockStorageApi.prepareDeveloperArtifactCleanup(result.scan_id, [item.id])).rejects.toThrow('cleanup is unavailable');
    }
    const rendered = render(DeveloperArtifactsView, { props: { onBack: () => undefined, initialResult: result } });
    expect(rendered.body).toContain('SvelteKit output');
    expect(rendered.body).toContain('Next.js output');
    expect(rendered.body).toContain('Observed only · cleanup unavailable');
    expect(rendered.body).toContain('Observed unit:');
    expect(rendered.body).not.toContain('only · source stays');
  });

  it('refuses incomplete ownership independently from a partial byte measurement', async () => {
    const result = await mockStorageApi.startDeveloperArtifactScan(['workspace-work'], () => undefined);
    const blocked = result.items.find((item) => item.status === 'safety_blocked')!;
    expect(blocked.ownership).toMatchObject({ state: 'incomplete', reason: 'unreadable_metadata' });
    expect(blocked.logical_bytes).toBeGreaterThan(0);
    const forgedStatus = { ...blocked, status: 'measurement_incomplete' as const };
    const rendered = render(DeveloperArtifactsView, { props: { onBack: () => undefined, initialResult: { ...result, items: [forgedStatus] } } });
    expect(rendered.body).toContain('disabled');
    expect(rendered.body).toContain('Partial measurement');
  });

  it('selects only the supported generated framework children while their parents stay observed', async () => {
    const result = await mockStorageApi.startDeveloperArtifactScan(['workspace-work'], () => undefined);
    const children = result.items.filter(item => item.kind === 'svelte_kit_types' || item.kind === 'next_webpack_cache');
    expect(children).toHaveLength(2);
    expect(children.every(item => item.status === 'complete' && !item.selected_by_default)).toBe(true);
    for (const child of children) {
      const preview = await mockStorageApi.prepareDeveloperArtifactCleanup(result.scan_id, [child.id]);
      expect(preview).toMatchObject({ item_count: 1, allocated_size: child.allocated_bytes });
    }
    const body = render(DeveloperArtifactsView, { props: { onBack: () => undefined, initialResult: result } }).body;
    expect(body).toContain('Next.js Webpack build cache'); expect(body).toContain('SvelteKit generated types');
    expect(body).toContain('parent deployment and offline output stay');
    expect(body).toContain('Observed only · cleanup unavailable');
  });

  it('rejects forged artifact IDs even when a valid ID is also selected', async () => {
    const result = await mockStorageApi.startDeveloperArtifactScan(['workspace-myproject'], () => undefined);
    const valid = result.items.find((item) => item.status === 'complete');
    expect(valid).toBeDefined();
    await expect(
      mockStorageApi.prepareDeveloperArtifactCleanup(result.scan_id, [valid!.id, 'forged-artifact'])
    ).rejects.toThrow('inventory changed');
  });

  it('scans the whole user scope without manually adding project folders', async () => {
    const workspace = await mockStorageApi.registerDeveloperHomeWorkspace();
    const result = await mockStorageApi.startDeveloperArtifactScan([workspace.id], () => undefined);

    expect(workspace.name).toBe('This Computer');
    expect(result.items.length).toBeGreaterThan(3);
    expect(result.items.every((item) => item.workspace_id === workspace.id)).toBe(true);
    expect(result.items.some((item) => item.ecosystem === 'kotlin')).toBe(true);
  });

  it('renders the review-only copy and supported ecosystem guidance', () => {
    const rendered = render(DeveloperArtifactsView, {
      props: { onBack: () => undefined },
    });

    expect(rendered.body).toContain('Developer Artifacts');
    expect(rendered.body).toContain('nothing selected by default');
    expect(rendered.body).toContain('Scan this computer');
    expect(rendered.body).toContain('System, credential, media, and installed-application paths are bypassed');
    expect(rendered.body).toContain('Project source, manifests, lockfiles, and project roots are never cleanup targets');
    expect(rendered.body).toContain('Java/Kotlin');
    expect(rendered.body).toContain('Terraform');
    expect(rendered.body).not.toContain('Incomplete · blocked');
  });

  it('aligns artifact sizes and keeps evidence behind a disclosure', async () => {
    const result = await mockStorageApi.startDeveloperArtifactScan(
      ['workspace-myproject'],
      () => undefined
    );
    const rendered = render(DeveloperArtifactsView, {
      props: { onBack: () => undefined, initialResult: result },
    });

    expect(rendered.body).toContain('developer-artifact-list');
    expect(rendered.body).toContain('developer-artifact-row');
    expect(rendered.body).toContain('allocated</span>');
    expect(rendered.body).toContain('<details');
    expect(rendered.body).toContain('Evidence and rebuild details');
  });

  it('reports the gated Downloads folder as uninspected in the whole-home preview', async () => {
    const workspace = await mockStorageApi.registerDeveloperHomeWorkspace();
    const events: string[] = [];
    const result = await mockStorageApi.startDeveloperArtifactScan([workspace.id], (event) => {
      events.push(event.type === 'uninspected' ? `uninspected:${event.name}` : event.type);
    });

    expect(result.uninspected).toMatchObject([
      { name: 'Downloads', path: '/Users/mock/Downloads', reason: 'permission_denied', retryable: true },
    ]);
    expect(result.skipped_entries).toBeGreaterThan(0);
    expect(events).toContain('uninspected:Downloads');
  });

  it('renders a refused folder notice with its reason and an actionable rescan', () => {
    const rendered = render(DeveloperArtifactsView, {
      props: {
        onBack: () => undefined,
        initialResult: {
          scan_id: 'mock-developer-scan-downloads',
          items: [],
          discovered_count: 0,
          measured_count: 0,
          skipped_entries: 1,
          cancelled: false,
          truncated: false,
          uninspected: [
            {
              path: '/Users/mock/Downloads',
              name: 'Downloads',
              reason: 'permission_denied',
              retryable: true,
            },
          ],
        },
      },
    });

    expect(rendered.body).toContain('role="status"');
    expect(rendered.body).toContain('data-testid="developer-artifact-uninspected-notice"');
    expect(rendered.body).toContain('Downloads');
    expect(rendered.body).toContain('/Users/mock/Downloads');
    expect(rendered.body).toContain('The operating system refused access, so this folder was not inspected.');
    expect(rendered.body).toContain('Permission refused · retry allowed');
    expect(rendered.body).toContain('System Settings');
    expect(rendered.body).toContain('Files and Folders');
    expect(rendered.body).toContain('Scan again');
    expect(rendered.body).toContain('1 uninspected');
    expect(rendered.body).toContain('Result is partial');
  });

  it('omits the permission guidance for an unreadable folder that a retry cannot include', () => {
    const rendered = render(DeveloperArtifactsView, {
      props: {
        onBack: () => undefined,
        initialResult: {
          scan_id: 'mock-developer-scan-unreadable',
          items: [],
          discovered_count: 0,
          measured_count: 0,
          skipped_entries: 1,
          cancelled: false,
          truncated: false,
          uninspected: [
            {
              path: '/Users/mock/work/broken',
              name: 'broken',
              reason: 'unreadable',
              retryable: false,
            },
          ],
        },
      },
    });

    expect(rendered.body).toContain('data-testid="developer-artifact-uninspected-notice"');
    expect(rendered.body).toContain('This folder could not be read, so it was not inspected.');
    expect(rendered.body).toContain('Unreadable');
    expect(rendered.body).not.toContain('System Settings');
    expect(rendered.body).not.toContain('Permission refused');
  });
});
