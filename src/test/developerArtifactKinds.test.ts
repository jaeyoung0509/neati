import { describe, expect, it } from 'vitest';
import type { DeveloperArtifactKind } from '../lib/models/types';
import { developerArtifactKindLabel } from '../lib/utils/developerArtifactKinds';

/** The project-local caches added for kondo and npkill parity (#479). */
const PROJECT_CACHES: DeveloperArtifactKind[] = [
  'python_bytecode_cache',
  'pytest_cache',
  'mypy_cache',
  'ruff_cache',
  'tox_environments',
  'nox_environments',
  'turbo_cache',
  'angular_cache',
  'nx_cache',
  'unity_library',
  'unity_temp',
  'unity_obj',
  'unity_logs',
  'unreal_intermediate',
  'unreal_derived_data_cache',
  'godot_cache',
  'elm_stuff',
  'clojure_cpcache',
  'bloop_cache',
  'metals_cache',
];

describe('developer artifact kind labels', () => {
  it('names every project-local cache once, without repeating another kind', () => {
    const labels = PROJECT_CACHES.map(developerArtifactKindLabel);
    for (const label of labels) expect(label.trim()).not.toBe('');
    expect(new Set(labels).size).toBe(labels.length);
    expect(labels).not.toContain(developerArtifactKindLabel('flutter_tooling'));
    expect(labels).not.toContain(developerArtifactKindLabel('dotnet_obj'));
  });
});
