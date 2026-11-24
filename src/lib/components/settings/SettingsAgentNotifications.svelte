<script lang="ts">
  import type { AgentNotificationPreferences } from '../../models/types';
  import { settingsStore } from '../../stores/settings.svelte';
  import Badge from '../Badge.svelte';
  import Card from '../Card.svelte';
  import Switch from '../Switch.svelte';

  let settings = $derived(settingsStore.settings);

  function handleNotificationToggle(key: keyof AgentNotificationPreferences) {
    const current = settings.agent_notifications ?? {
      enabled: false,
      notify_on_turn_completed: true,
      notify_on_approval_or_input: true,
      notify_on_possibly_inactive: true,
      hide_project_basename: false,
      inactivity_threshold_minutes: 15,
    };
    const updated = {
      ...current,
      [key]: !current[key],
    };
    settingsStore.save({ agent_notifications: updated });
  }

  function handleThresholdChange(minutes: number) {
    const current = settings.agent_notifications ?? {
      enabled: false,
      notify_on_turn_completed: true,
      notify_on_approval_or_input: true,
      notify_on_possibly_inactive: true,
      hide_project_basename: false,
      inactivity_threshold_minutes: 15,
    };
    const updated = {
      ...current,
      inactivity_threshold_minutes: Math.max(5, Math.min(120, minutes)),
    };
    settingsStore.save({ agent_notifications: updated });
  }

</script>

<!-- Agent Activity Notifications -->
<div class="space-y-3">
  <div class="flex items-center justify-between">
    <h3 class="text-meta font-semibold text-muted-foreground tracking-normal">
      Agent notifications
    </h3>
    <Badge variant="outline">Privacy Safe</Badge>
  </div>
  <Card class="p-4 space-y-4 bg-card divide-y divide-border/60">
    <!-- Master toggle -->
    <div class="flex items-center justify-between text-meta pt-3 first:pt-0">
      <div>
        <div class="font-medium text-foreground">Enable Desktop Notifications</div>
        <div class="text-meta text-muted-foreground">Opt-in notifications for agent lifecycle and user attention events.</div>
      </div>
      <Switch
        checked={settings.agent_notifications?.enabled ?? false}
        onchange={() => handleNotificationToggle('enabled')}
        ariaLabel="Enable Desktop Notifications"
      />
    </div>

    {#if settings.agent_notifications?.enabled}
      <!-- Turn completed -->
      <div class="flex items-center justify-between text-meta pt-3">
        <div>
          <div class="font-medium text-foreground">Turn Completed</div>
          <div class="text-meta text-muted-foreground">Notify when an agent finishes its active response turn.</div>
        </div>
        <Switch
          checked={settings.agent_notifications?.notify_on_turn_completed ?? true}
          onchange={() => handleNotificationToggle('notify_on_turn_completed')}
          ariaLabel="Notify on Turn Completed"
        />
      </div>

      <!-- Approval or input needed -->
      <div class="flex items-center justify-between text-meta pt-3">
        <div>
          <div class="font-medium text-foreground">Needs Approval or Input</div>
          <div class="text-meta text-muted-foreground">Notify when an agent is waiting for confirmation, tool permission, or user input.</div>
        </div>
        <Switch
          checked={settings.agent_notifications?.notify_on_approval_or_input ?? true}
          onchange={() => handleNotificationToggle('notify_on_approval_or_input')}
          ariaLabel="Notify on Approval or Input"
        />
      </div>

      <!-- Possibly inactive -->
      <div class="flex items-center justify-between text-meta pt-3">
        <div>
          <div class="font-medium text-foreground">Possibly Inactive</div>
          <div class="text-meta text-muted-foreground">Notify if an agent has been running with no observable activity past the threshold.</div>
        </div>
        <Switch
          checked={settings.agent_notifications?.notify_on_possibly_inactive ?? true}
          onchange={() => handleNotificationToggle('notify_on_possibly_inactive')}
          ariaLabel="Notify on Possibly Inactive"
        />
      </div>

      <!-- Hide project basename -->
      <div class="flex items-center justify-between text-meta pt-3">
        <div>
          <div class="font-medium text-foreground">Hide Project Name in Notifications</div>
          <div class="text-meta text-muted-foreground">Replaces the project folder name with "an active project" in notification body text.</div>
        </div>
        <Switch
          checked={settings.agent_notifications?.hide_project_basename ?? false}
          onchange={() => handleNotificationToggle('hide_project_basename')}
          ariaLabel="Hide Project Name in Notifications"
        />
      </div>

      <!-- Inactivity threshold slider -->
      <div class="text-meta pt-3 space-y-2">
        <div class="flex items-center justify-between">
          <span class="font-medium text-foreground">Inactivity Alert Threshold</span>
          <span class="font-mono text-muted-foreground">{settings.agent_notifications?.inactivity_threshold_minutes ?? 15} minutes</span>
        </div>
        <input
          type="range"
          min="5"
          max="60"
          step="5"
          value={settings.agent_notifications?.inactivity_threshold_minutes ?? 15}
          oninput={(e) => handleThresholdChange(Number((e.target as HTMLInputElement).value))}
          class="w-full h-1.5 bg-secondary rounded-lg appearance-none cursor-pointer accent-control"
        />
      </div>
    {/if}
  </Card>
</div>
