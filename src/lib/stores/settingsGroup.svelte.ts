/** Settings shows one group at a time (#519). */
export type SettingsGroup = 'cleanup' | 'agents' | 'app';

export const SETTINGS_GROUPS: { id: SettingsGroup; label: string }[] = [
  { id: 'cleanup', label: 'Cleanup' },
  { id: 'agents', label: 'Agents' },
  { id: 'app', label: 'App' },
];

/** The open Settings group. A view that sends the person to one setting sets it first. */
class SettingsGroupStore {
  current = $state<SettingsGroup>('cleanup');
}

export const settingsGroupStore = new SettingsGroupStore();
