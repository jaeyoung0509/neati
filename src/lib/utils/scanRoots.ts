/**
 * Owner providers report their step as a `provider://<id>` root (#493). It
 * names a tool step, not a location, so it is never shown as a path.
 */
export const PROVIDER_ROOT_SCHEME = 'provider://';

export function isFilesystemRoot(root: string): boolean {
  return !root.startsWith(PROVIDER_ROOT_SCHEME);
}
