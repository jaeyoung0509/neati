import type { DeveloperArtifact } from '../models/types';

// Display selection only. The backend independently binds and authorizes scope.
function pathKey(path: string): string {
  const normalized = path.replace(/\\/g, '/').replace(/\/+$/, '');
  return /^[a-z]:\//i.test(normalized) || normalized.startsWith('//')
    ? normalized.toLowerCase()
    : normalized;
}

export function artifactPathsOverlap(left: string, right: string): boolean {
  const a = pathKey(left);
  const b = pathKey(right);
  return a === b || a.startsWith(`${b}/`) || b.startsWith(`${a}/`);
}

export function toggleArtifactSelection(selected: string[], item: DeveloperArtifact, items: DeveloperArtifact[]): string[] {
  if (selected.includes(item.id)) return selected.filter(id => id !== item.id);
  return [...selected.filter(id => {
    const other = items.find(candidate => candidate.id === id);
    return !other || !artifactPathsOverlap(item.path, other.path);
  }), item.id];
}

export function nonOverlappingArtifactIds(items: DeveloperArtifact[]): string[] {
  const selected: DeveloperArtifact[] = [];
  for (const item of [...items].sort((a, b) => pathKey(a.path).length - pathKey(b.path).length)) {
    if (!selected.some(other => artifactPathsOverlap(item.path, other.path))) selected.push(item);
  }
  return selected.map(item => item.id);
}

export function remainingArtifactScopes(items: DeveloperArtifact[], movedIds: Set<string>): DeveloperArtifact[] {
  const movedPaths = items.filter(item => movedIds.has(item.id)).map(item => item.path);
  return items.filter(item => !movedPaths.some(path => artifactPathsOverlap(item.path, path)));
}
