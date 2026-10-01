import type { DeveloperArtifact } from '../../models/types';
import type { PreviewPlatform } from '../../models/platformContext';

const MIB = 1024 * 1024;
const base = {
  workspace_id: 'workspace-myproject', ecosystem: 'node' as const,
  ownership: { state: 'verified_generated' as const }, selected_by_default: false,
  newest_mtime: null, incomplete_reason: null, status: 'complete' as const,
};

// Response fixtures, never an implementation of native format authorization.
export const frameworkArtifactFixtures: DeveloperArtifact[] = [
  ...(['svelte_kit_output', 'next_output'] as const).flatMap(kind => {
    const svelte = kind === 'svelte_kit_output';
    const project = svelte ? 'sync-default' : 'cache-default';
    const path = `/Users/mock/Myproject/${project}/${svelte ? '.svelte-kit' : '.next'}`;
    return [{ ...base, id: `whole-${kind}`, project_name: project, kind, path,
      logical_bytes: 20 * MIB, allocated_bytes: 18 * MIB, file_count: 15,
      evidence: [`Browser fixture: recorded ${svelte ? 'SvelteKit 2.37.1 sync-only' : 'Next 15.5.14 Webpack-cache-only'} whole output; complete idle project observation.`],
      rebuild_hint: 'The whole verified generated output moves to Trash. Project source and manifests stay; sync/build regenerates this output.',
    }, { ...base, id: `whole-child-${kind}`, project_name: project,
      kind: svelte ? 'svelte_kit_types' as const : 'next_webpack_cache' as const,
      path: `${path}/${svelte ? 'types' : 'cache/webpack'}`, logical_bytes: 16 * MIB,
      allocated_bytes: 15 * MIB, file_count: 10,
      evidence: ['Browser fixture: verified generated child inside an eligible whole default.'],
      rebuild_hint: 'Only this generated child moves to Trash. Choose this child or its whole parent.',
    }];
  }),
  { ...base, id: 'custom-next-output', project_name: 'custom-layout', kind: 'next_output',
    path: '/Users/mock/Myproject/custom-layout/generated/site', logical_bytes: 12 * MIB,
    allocated_bytes: 10 * MIB, file_count: 4, status: 'observation_only',
    evidence: ['Bounded distDir string observed in config; project JavaScript was not executed.'],
    incomplete_reason: 'Custom or unknown configuration grants no cleanup authority.',
    rebuild_hint: 'Observed custom output. Rebuild and deployment/offline ownership are unverified; cleanup is unavailable.',
  },
];

export function frameworkArtifactForPreview(artifact: DeveloperArtifact, platform: PreviewPlatform): DeveloperArtifact {
  if (platform === 'macos' || artifact.status !== 'complete') return artifact;
  const parent = artifact.kind === 'svelte_kit_output' || artifact.kind === 'next_output';
  if (!parent && artifact.kind !== 'svelte_kit_types' && artifact.kind !== 'next_webpack_cache') return artifact;
  return { ...artifact, status: parent ? 'observation_only' : 'safety_blocked',
    incomplete_reason: 'This platform has no implementing framework project-use and removal adapter.',
    evidence: [...artifact.evidence, 'Browser fixture: native framework mutation is unavailable on this platform.'],
  };
}
