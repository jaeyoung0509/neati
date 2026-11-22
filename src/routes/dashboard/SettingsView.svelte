<script lang="ts">
  import CleanupGuidanceList from '../../lib/components/CleanupGuidanceList.svelte';
  import { scanStore } from '../../lib/stores/scan.svelte';
  import type { CategoryResult } from '../../lib/models/types';
  let { onCategory, onNavigate }: { onCategory?: (category: CategoryResult) => void; onNavigate?: (tab: DashboardTab) => void } = $props();
  import { onMount } from 'svelte';
  import { settingsStore } from '../../lib/stores/settings.svelte';
  import { platformCapabilitiesStore } from '../../lib/stores/platformCapabilities.svelte';
  import { platformContextStore } from '../../lib/stores/platformContext.svelte';
  import { usageStore } from '../../lib/stores/usage.svelte';
  import type { DashboardTab } from '../../lib/models/types';
  import Card from '../../lib/components/Card.svelte';
  import Badge from '../../lib/components/Badge.svelte';
  import Button from '../../lib/components/Button.svelte';
  import Switch from '../../lib/components/Switch.svelte';
  import Checkbox from '../../lib/components/Checkbox.svelte';
  import ReorderControls from '../../lib/components/ReorderControls.svelte';
  import PageHeader from '../../lib/components/PageHeader.svelte';
  import InlineNotice from '../../lib/components/InlineNotice.svelte';
  import UpdateSettings from '../../lib/components/UpdateSettings.svelte';
  import LaunchAtLoginSetting from '../../lib/components/LaunchAtLoginSetting.svelte';
  import CleanupHistory from '../../lib/components/CleanupHistory.svelte';
  import AiToolProfiles from '../../lib/components/AiToolProfiles.svelte';
  import SettingsDiagnostics from '../../lib/components/settings/SettingsDiagnostics.svelte';
  import SettingsQuickPanel from '../../lib/components/settings/SettingsQuickPanel.svelte';
  import SettingsAiAccounts from '../../lib/components/settings/SettingsAiAccounts.svelte';
  import SettingsAgentNotifications from '../../lib/components/settings/SettingsAgentNotifications.svelte';
  import StorageAccessSetup from '../../lib/components/StorageAccessSetup.svelte';
  import ToolDetectionDisclosure from '../../lib/components/ai-activity/ToolDetectionDisclosure.svelte';
  import { APP_VERSION, formatVersion } from '../../lib/utils/version';
  import { Settings, Moon, Sun, Monitor, AlertTriangle } from '@lucide/svelte';

  let settings = $derived(settingsStore.settings);

  let releasesUrl = $derived.by(() => {
    const url = platformContextStore.context?.releases_url;
    return typeof url === 'string' && url.length > 0 ? url : null;
  });

  // Derived from the one release URL the backend states. No query string: the
  // person pastes the report themselves, so nothing personal rides in a URL.
  let issueUrl = $derived(releasesUrl ? releasesUrl.replace(/\/releases\/?$/, '/issues/new') : null);

  let isIntensiveAvailable = $derived(
    platformCapabilitiesStore.isAvailable('intensive_cleanup')
  );
  let intensiveReason = $derived(
    platformCapabilitiesStore.feature('intensive_cleanup')?.reason
  );




  function handleToggle(key: keyof typeof settings) {
    if (typeof settings[key] === 'boolean') {
      settingsStore.save({ [key]: !settings[key] });
    }
  }

  function handleTheme(theme: string) {
    settingsStore.save({ theme });
  }

  onMount(() => {
    void platformContextStore.load();
    void usageStore.loadDescriptors();
  });

</script>

<div class="space-y-6 max-w-2xl">
  <!-- Page Header -->
  <PageHeader
    title="Settings"
    subtitle="Choose how neati looks, cleans, and works with your tools."
    icon={Settings}
  />

  {#if settingsStore.error}
    <InlineNotice
      variant="destructive"
      title="Settings Error"
      message={settingsStore.error}
    />
  {/if}

  <!-- Appearance -->
  <div class="space-y-3">
    <h3 class="text-meta font-semibold text-muted-foreground tracking-normal">
      Appearance
    </h3>
    <Card class="p-4 bg-card">
      <div class="grid grid-cols-3 gap-3">
        <button
          type="button"
          onclick={() => handleTheme('system')}
          aria-pressed={settings.theme === 'system'}
          class="flex flex-col items-center justify-center p-3 rounded-lg border text-meta gap-2 transition-[background-color,color,border-color] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring {settings.theme ===
          'system'
            ? 'border-primary bg-secondary/80 text-foreground font-semibold'
            : 'border-border text-muted-foreground hover:text-foreground'}"
        >
          <Monitor size={18} aria-hidden="true" />
          <span>System</span>
        </button>

        <button
          type="button"
          onclick={() => handleTheme('dark')}
          aria-pressed={settings.theme === 'dark'}
          class="flex flex-col items-center justify-center p-3 rounded-lg border text-meta gap-2 transition-[background-color,color,border-color] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring {settings.theme ===
          'dark'
            ? 'border-primary bg-secondary/80 text-foreground font-semibold'
            : 'border-border text-muted-foreground hover:text-foreground'}"
        >
          <Moon size={18} aria-hidden="true" />
          <span>Dark</span>
        </button>

        <button
          type="button"
          onclick={() => handleTheme('light')}
          aria-pressed={settings.theme === 'light'}
          class="flex flex-col items-center justify-center p-3 rounded-lg border text-meta gap-2 transition-[background-color,color,border-color] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring {settings.theme ===
          'light'
            ? 'border-primary bg-secondary/80 text-foreground font-semibold'
            : 'border-border text-muted-foreground hover:text-foreground'}"
        >
          <Sun size={18} aria-hidden="true" />
          <span>Light</span>
        </button>
      </div>
    </Card>
  </div>

  <!-- General Preferences -->
  <div class="space-y-3">
    <h3 class="text-meta font-semibold text-muted-foreground tracking-normal">
      General
    </h3>
    <Card class="p-4 space-y-4 bg-card">
      <LaunchAtLoginSetting />
      <div class="flex items-center justify-between gap-3 text-meta">
        <div>
          <div class="font-medium text-foreground">Welcome guide</div>
          <div class="text-muted-foreground">What neati cleans, what permanent removal means, and optional setup.</div>
        </div>
        <Button size="sm" variant="outline" onclick={() => void settingsStore.save({ onboarding_completed: false })}>Show guide</Button>
      </div>
    </Card>
  </div>

  <!-- Updates -->
  <div class="space-y-3">
    <h3 class="text-meta font-semibold text-muted-foreground tracking-normal">
      Updates
    </h3>
    <Card class="p-4 bg-card">
      <UpdateSettings {releasesUrl} />
    </Card>
  </div>

  <!-- AI tool profiles -->
  <div class="space-y-3">
    <h3 class="text-meta font-semibold text-muted-foreground tracking-normal">
      AI tool profiles
    </h3>
    <Card class="p-4 bg-card">
      <AiToolProfiles />
    </Card>
  </div>

  <SettingsAiAccounts />

  <SettingsQuickPanel />

  <!-- Cleanup Scan Scope -->
  <div class="space-y-3">
    <div>
      <h3 class="text-meta font-semibold text-muted-foreground tracking-normal">
        Cache scanning
      </h3>
      <p class="text-meta text-muted-foreground mt-1">
        Verified cache and log locations are included in every storage scan.
      </p>
    </div>
    <Card class="p-4 bg-card">
      {#if platformCapabilitiesStore.error && !platformCapabilitiesStore.capabilities}
        <InlineNotice
          variant="destructive"
          title="Platform capabilities unavailable"
          message={platformCapabilitiesStore.error}
          actionLabel="Retry"
          onAction={() => void platformCapabilitiesStore.load(true)}
        />
      {:else}
        <div class="flex items-start justify-between gap-5 text-meta">
          <div class="min-w-0">
            <div class="flex items-center gap-2 font-medium text-foreground">
              <AlertTriangle size={14} class="text-warning" />
              Extended cache scanning
              <Badge variant="outline">{isIntensiveAvailable ? 'Always included' : 'Unavailable'}</Badge>
            </div>
            <div class="text-meta text-muted-foreground mt-1 leading-relaxed">
              {#if !isIntensiveAvailable && intensiveReason}
                {intensiveReason}
              {:else}
                Scans include regenerable Apple and third-party cache payloads without an age delay, plus stale logs. DotSlash artifacts unchanged for 30 days
                become available for individual review; their modification time does not show when they were last used.
                Selected artifacts move to Trash; empty it to free disk space. DotSlash may download them again.
                Personal files,
                settings, credentials, databases, and compiled model stores remain protected.
                Scanning never deletes anything automatically.
              {/if}
            </div>
          </div>
        </div>
      {/if}
    </Card>
  </div>

  {#if platformContextStore.context?.platform === 'macos'}
    <Card class="p-4 bg-card">
      <StorageAccessSetup />
    </Card>
  {/if}

  {#if scanStore.lastScan}
    <CleanupGuidanceList scan={scanStore.lastScan} {onCategory} {onNavigate} current={scanStore.canClean} />
  {/if}

  <!-- Cleaning Categories Defaults -->
  <div class="space-y-3">
    <h3 class="text-meta font-semibold text-muted-foreground tracking-normal">
      Quick Clean categories
    </h3>
    <Card class="p-4 space-y-4 bg-card divide-y divide-border/60">
      <!-- AI Tools -->
      <div class="flex items-center justify-between text-meta pt-3 first:pt-0">
        <div>
          <div class="font-medium text-foreground">AI Assistant Caches & Logs</div>
          <div class="text-meta text-muted-foreground">Claude Code, Cursor, Gemini CLI, Codex, Aider.</div>
        </div>
        <Switch
          checked={settings.clean_ai_tools}
          onchange={() => handleToggle('clean_ai_tools')}
          ariaLabel="AI Assistant Caches & Logs"
        />
      </div>

      <!-- Developer Tools -->
      <div class="flex items-center justify-between text-meta pt-3">
        <div>
          <div class="font-medium text-foreground">Developer Compilers & Package Managers</div>
          <div class="text-meta text-muted-foreground">Go build, Cargo cache, npm, pnpm, uv, Xcode DerivedData.</div>
        </div>
        <Switch
          checked={settings.clean_developer_tools}
          onchange={() => handleToggle('clean_developer_tools')}
          ariaLabel="Developer Compilers & Package Managers"
        />
      </div>

      <!-- Docker -->
      <div class="flex items-center justify-between text-meta pt-3">
        <div>
          <div class="font-medium text-foreground">Docker Dangling Images & BuildKit Cache</div>
          <div class="text-meta text-muted-foreground">Clean safe Docker cache layers via official Docker CLI.</div>
        </div>
        <Switch
          checked={settings.clean_docker}
          onchange={() => handleToggle('clean_docker')}
          ariaLabel="Docker Dangling Images & BuildKit Cache"
        />
      </div>

    </Card>
  </div>

  <SettingsAgentNotifications />

  <!-- Cleanup history -->
  <div class="space-y-3">
    <h3 class="text-meta font-semibold text-muted-foreground tracking-normal">
      Cleanup history
    </h3>
    <Card class="p-4 bg-card">
      <CleanupHistory />
    </Card>
  </div>

  <SettingsDiagnostics {issueUrl} />

  <!-- About -->
  <div class="space-y-3 pt-2">
    <h3 class="text-meta font-semibold text-muted-foreground tracking-normal">
      About neati
    </h3>
    <Card class="p-4 bg-card text-meta space-y-2">
      <div class="flex items-center justify-between gap-3">
        <span class="font-medium text-foreground">neati Developer System Manager</span>
        <Badge variant="outline" class="font-mono">{formatVersion(APP_VERSION)}</Badge>
      </div>
      {#if releasesUrl}
        <a
          class="inline-block text-primary underline underline-offset-2 text-meta"
          href={releasesUrl}
          target="_blank"
          rel="noreferrer noopener"
          title="Opens the release page in your browser"
        >Check for a newer release</a>
      {/if}
      <p class="text-muted-foreground leading-relaxed">
        neati is an ultra-lightweight open-source utility designed to safely manage AI caches, developer build artifacts, Docker storage, local LLMs, memory pressure, and keep-awake power assertions.
      </p>
      <div class="pt-2 text-meta text-muted-foreground font-mono">
        Built with Tauri 2 + Svelte 5 + Rust. Zero analytics, zero cloud, 100% local.
      </div>
    </Card>
  </div>
</div>
