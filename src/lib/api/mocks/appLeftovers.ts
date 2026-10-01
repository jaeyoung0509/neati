import type { AppLeftoverInventory } from '../../models/types';
import type { PreviewPlatform } from '../../models/platformContext';

export const mockAppLeftoverInventory: AppLeftoverInventory = {
      items: [
        { id: 'leftover-example-cache', name: 'com.example.previous-app', display_path: '/Users/mock/Library/Caches/com.example.previous-app', kind: 'cache', classification: 'possible_removed_owner', owner_names: [], evidence: 'No matching bundle was found in the checked Applications folders. Relocated apps, unavailable volumes, helpers and command-line owners are not excluded.', logical_size: 24 * 1024 * 1024, allocated_size: 24 * 1024 * 1024, quality: 'fresh', incomplete_reason: null },
        { id: 'leftover-shared', name: 'group.com.example.shared', display_path: '/Users/mock/Library/Group Containers/group.com.example.shared', kind: 'group_container', classification: 'ambiguous_shared_owner', owner_names: [], evidence: 'A shared container may still serve helpers or other applications. Cleanup unavailable.', logical_size: 18 * 1024 * 1024, allocated_size: 18 * 1024 * 1024, quality: 'partial', incomplete_reason: 'A shared-container descendant could not be measured; its displayed bytes are a lower bound.' },
        { id: 'leftover-protected', name: 'PreviousApp', display_path: '/Users/mock/Library/Application Support/PreviousApp', kind: 'application_support', classification: 'protected_state', owner_names: [], evidence: 'May contain settings, databases, credentials or offline state. Missing an application does not authorize removal.', logical_size: 12 * 1024 * 1024, allocated_size: 12 * 1024 * 1024, quality: 'fresh', incomplete_reason: null },
        { id: 'leftover-installed', name: 'com.microsoft.VSCode', display_path: '/Users/mock/Library/Caches/com.microsoft.VSCode', kind: 'cache', classification: 'installed_owner', owner_names: ['Visual Studio Code'], evidence: 'A bundle identifier match was found in the freshly checked application folders. Helpers and command-line owners may also use this namespace.', logical_size: 8 * 1024 * 1024, allocated_size: 8 * 1024 * 1024, quality: 'fresh', incomplete_reason: null },
      ], quality: 'partial', observed_roots: 6, skipped_entry_count: 1, incomplete_reasons: ['A shared-container descendant could not be measured; its displayed bytes are a lower bound.'],
      limitation: 'Read-only observations, separate from Cleanup estimates. A missing owner does not prove uninstall: apps may be relocated, portable, on an unavailable volume, or command-line tools. Cleanup is unavailable.',
    };

export function mockAppLeftoversForPlatform(platform: PreviewPlatform): AppLeftoverInventory {
  if (platform === 'macos') return structuredClone(mockAppLeftoverInventory);
  return {
    items: [], quality: 'unavailable', observed_roots: 0, skipped_entry_count: 0,
    incomplete_reasons: ['User Library resource review has a macOS adapter only'],
    limitation: mockAppLeftoverInventory.limitation,
  };
}
