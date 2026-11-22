import { afterEach, describe, expect, it, vi } from 'vitest';
import { render } from 'svelte/server';
import { createRequire } from 'node:module';
import OverviewView from '../routes/dashboard/OverviewView.svelte';
import QuickPanel from '../routes/quick/QuickPanel.svelte';
import { mockStorageApi } from '../lib/api/storage';
import { settingsStore } from '../lib/stores/settings.svelte';
import { platformCapabilitiesStore } from '../lib/stores/platformCapabilities.svelte';
import { goldenCapabilitiesByPlatform } from '../lib/models/platformCapabilities';

// Keep Svelte's server-rendering environment; use jsdom only as an HTML parser.
const { JSDOM } = createRequire(import.meta.url)('jsdom') as {
  JSDOM: new (html: string) => { window: { document: Document } };
};
const markupDocument = (body: string): Document => new JSDOM(body).window.document;

function stubPreviewSettings(projectFolders: string[]) {
  const values = new Map([['neati.settings', JSON.stringify({ project_folders: projectFolders })]]);
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => values.set(key, value),
    removeItem: (key: string) => values.delete(key),
  });
}

function withSettings<T>(projectFolders: string[], body: () => T): T {
  const previousSettings = settingsStore.settings;
  const previousLoaded = settingsStore.hasLoaded;
  try {
    platformCapabilitiesStore.capabilities = goldenCapabilitiesByPlatform.macos;
    settingsStore.hasLoaded = true;
    settingsStore.settings = {
      ...previousSettings,
      quick_panel_sections: ['cleanup'],
      project_folders: projectFolders,
    };
    return body();
  } finally {
    settingsStore.settings = previousSettings;
    settingsStore.hasLoaded = previousLoaded;
  }
}

afterEach(() => {
  vi.unstubAllGlobals();
  platformCapabilitiesStore.reset();
});

describe('saved project folders (#486)', () => {
  it('suggests an unsaved project root and restores only saved folders', async () => {
    stubPreviewSettings([]);
    const suggestions = await mockStorageApi.suggestDeveloperWorkspaces();
    expect(suggestions.map((suggestion) => suggestion.path)).toEqual(['/Users/mock/Myproject']);
    expect((await mockStorageApi.restoreDeveloperWorkspaces()).workspaces).toEqual([]);

    stubPreviewSettings(['/Users/mock/Myproject']);
    expect(await mockStorageApi.suggestDeveloperWorkspaces()).toEqual([]);
    const restore = await mockStorageApi.restoreDeveloperWorkspaces();
    expect(restore.workspaces.map((workspace) => workspace.path)).toEqual(['/Users/mock/Myproject']);
    expect(restore.refused).toEqual([]);
  });

  it('adds only a folder that was suggested', async () => {
    stubPreviewSettings([]);
    const workspace = await mockStorageApi.addSuggestedDeveloperWorkspace('/Users/mock/Myproject');
    expect(workspace.path).toBe('/Users/mock/Myproject');
    await expect(mockStorageApi.addSuggestedDeveloperWorkspace('/Users/mock/work')).rejects.toThrow(
      /no longer a suggestion/
    );
  });

  it('tells Home and the Quick Panel that project folders are not scanned until one is saved', () => {
    withSettings([], () => {
      const overview = markupDocument(render(OverviewView).body);
      expect(overview.body.textContent).toContain('Project build folders are not scanned yet.');
      expect(
        Array.from(overview.querySelectorAll('button')).some(
          (button) => button.textContent?.trim() === 'Add a projects folder'
        )
      ).toBe(true);

      const quick = markupDocument(render(QuickPanel).body);
      const summary = quick.querySelector('#quick-cleanup-summary')!;
      const hint = summary.querySelector('.quick-project-folders-hint');
      expect(hint?.textContent).toContain('Project folders not scanned');
      // The summary's first control stays its cleanup action.
      expect(summary.querySelector('button')).not.toBe(hint);
    });

    withSettings(['/Users/mock/Myproject'], () => {
      expect(render(OverviewView).body).not.toContain('Project build folders are not scanned yet.');
      expect(render(QuickPanel).body).not.toContain('Project folders not scanned');
    });
  });
});
