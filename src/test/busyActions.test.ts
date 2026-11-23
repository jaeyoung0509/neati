import { readFileSync } from 'node:fs';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { render } from 'svelte/server';
import AiUsageCards from '../lib/components/AiUsageCards.svelte';
import AwakeView from '../routes/dashboard/AwakeView.svelte';
import { UsageStore } from '../lib/stores/usage.svelte';
import { AwakeStore, awakeStore } from '../lib/stores/awake.svelte';
import { settingsStore } from '../lib/stores/settings.svelte';
import { OPENROUTER_SIGN_IN_TIMEOUT_SECONDS, openRouterWaitHint } from '../lib/utils/providerSignIn';
import {
  tauriConnectOpenRouter,
  tauriGetAwakeState,
  tauriSetManualAwake,
} from '../lib/utils/tauri';
import type { AiProviderUsage, AwakeState } from '../lib/models/types';
import { buttonTag, inputTags, isDisabled } from './fixtures/markup';

vi.mock('../lib/utils/tauri', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../lib/utils/tauri')>();
  return {
    ...actual,
    tauriConnectOpenRouter: vi.fn(),
    tauriGetAwakeState: vi.fn(),
    tauriSetManualAwake: vi.fn(),
  };
});

const T0 = 1_800_000_000_000;

function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((done) => { resolve = done; });
  return { promise, resolve };
}

const openRouter: AiProviderUsage = {
  id: 'openrouter',
  name: 'OpenRouter',
  installed: true,
  connected: false,
  auth_label: 'OAuth PKCE',
  status_message: 'Connect to read usage.',
  support: 'manual',
  windows: [],
  summary: {
    lifetime_tokens: null, last_7d_tokens: null, peak_daily_tokens: null, current_streak_days: null,
    local_sessions: null, local_cost_usd: null, usage_usd: null, limit_remaining_usd: null,
  },
  action_url: null,
};

// A browser OAuth wait and Keep Awake saves no longer look frozen (#491).
describe('OpenRouter sign-in wait', () => {
  afterEach(() => vi.useRealTimers());

  it('names the backend timeout it waits under', () => {
    const rust = readFileSync(new URL('../../src-tauri/src/ai_providers/openrouter.rs', import.meta.url), 'utf8');
    const seconds = Number(rust.match(/const OAUTH_TIMEOUT: Duration = Duration::from_secs\((\d+)\);/)?.[1]);
    expect(seconds).toBe(OPENROUTER_SIGN_IN_TIMEOUT_SECONDS);
    expect(openRouterWaitHint()).toBe('Finish sign-in in your browser. neati stops waiting after 3 min.');
  });

  it('ignores a second connect while the browser wait is open', async () => {
    vi.useFakeTimers();
    vi.setSystemTime(T0);
    const wait = deferred();
    vi.mocked(tauriConnectOpenRouter).mockReturnValue(wait.promise);
    const store = new UsageStore();
    const first = store.connectOpenRouter();
    expect(store.openRouterAction).toEqual({ kind: 'connecting', startedAt: T0 });
    await store.connectOpenRouter();
    await store.disconnectOpenRouter();
    expect(tauriConnectOpenRouter).toHaveBeenCalledTimes(1);
    wait.resolve();
    await first;
    expect(store.openRouterAction).toBeNull();
  });

  it('shows the wait with elapsed seconds and a disabled connect button', () => {
    vi.useFakeTimers();
    vi.setSystemTime(T0 + 12_000);
    const body = render(AiUsageCards, {
      props: {
        providers: [openRouter],
        openRouterAction: { kind: 'connecting', startedAt: T0 },
        onConnectOpenRouter: () => undefined,
      },
    }).body;
    expect(body).toContain('Waiting for OpenRouter…');
    expect(body).toContain('12 s');
    expect(body).toContain('neati stops waiting after 3 min.');
    const connect = buttonTag(body, 'Connect with OpenRouter');
    expect(isDisabled(connect)).toBe(true);
    expect(connect).toContain('aria-busy="true"');
    expect(body).toContain('Waiting for browser…');
  });

  it('shows no wait before connect is pressed', () => {
    const body = render(AiUsageCards, {
      props: { providers: [openRouter], openRouterAction: null, onConnectOpenRouter: () => undefined },
    }).body;
    expect(body).not.toContain('Waiting for OpenRouter…');
    expect(isDisabled(buttonTag(body, 'Connect with OpenRouter'))).toBe(false);
  });
});

const awakeIdle: AwakeState = {
  is_active: false, behavior: null, trigger_source: null, active_process_name: null, active_rule_id: null,
  manual_expires_at: null, active_rules_count: 0, power_source: 'ac', last_error: null, rule_evaluations: [],
  evaluation_health: { status: 'healthy', last_completed_at: null, last_failed_at: null, last_failure_reason: null },
};

describe('Keep Awake busy states', () => {
  beforeEach(() => {
    vi.mocked(tauriGetAwakeState).mockResolvedValue(awakeIdle);
  });
  afterEach(() => {
    vi.restoreAllMocks();
    awakeStore.pending = null;
    awakeStore.loadPhase = 'idle';
    awakeStore.readError = null;
    awakeStore.error = null;
    settingsStore.hasLoaded = false;
  });

  it('runs one manual change at a time', async () => {
    const start = deferred();
    vi.mocked(tauriSetManualAwake).mockReturnValue(start.promise);
    const store = new AwakeStore();
    const first = store.setManual(1800);
    expect(store.pending).toEqual({ kind: 'manual', durationSecs: 1800 });
    expect(store.isLoading).toBe(true);
    await store.setManual(3600);
    await store.disableManual();
    expect(tauriSetManualAwake).toHaveBeenCalledTimes(1);
    start.resolve();
    await first;
    expect(store.pending).toBeNull();
    expect(store.loadPhase).toBe('ready');
  });

  it('ignores a second rule save while one is in flight', async () => {
    const save = deferred();
    const spy = vi.spyOn(settingsStore, 'save').mockImplementation(() => save.promise.then(() => true));
    const store = new AwakeStore();
    const first = store.toggleRule('rule.codex');
    expect(store.pending).toEqual({ kind: 'rule', ruleId: 'rule.codex', action: 'toggle' });
    expect(await store.deleteRule('rule.claude')).toBe(false);
    expect(await store.enableRecommendedRules()).toBe(false);
    expect(spy).toHaveBeenCalledTimes(1);
    save.resolve();
    expect(await first).toBe(true);
    expect(store.pending).toBeNull();
  });

  it('keeps the first state read from reading as final', () => {
    awakeStore.loadPhase = 'idle';
    const body = render(AwakeView).body;
    expect(body).toContain('Checking Keep Awake…');
    expect(body).not.toContain('Normal sleep is active');
    // Durations wait for the current state before they can be pressed.
    expect(isDisabled(buttonTag(body, '30 Minutes'))).toBe(true);

    awakeStore.loadPhase = 'ready';
    const ready = render(AwakeView).body;
    expect(ready).toContain('Normal sleep is active');
    expect(ready).not.toContain('Checking Keep Awake…');
    expect(isDisabled(buttonTag(ready, '30 Minutes'))).toBe(false);
  });

  it('keeps a failed first read apart from a failed change', async () => {
    vi.mocked(tauriGetAwakeState).mockRejectedValueOnce(new Error('power assertion query failed'));
    const store = new AwakeStore();
    await store.refresh();
    expect(store.loadPhase).toBe('error');
    expect(store.readError).toContain('power assertion query failed');
    expect(store.error).toBeNull();
    await store.refresh();
    expect(store.loadPhase).toBe('ready');
    expect(store.readError).toBeNull();

    awakeStore.loadPhase = 'error';
    awakeStore.readError = 'power assertion query failed';
    const body = render(AwakeView).body;
    expect(body).toContain('Keep Awake state unknown');
    expect(body).toContain('power assertion query failed');
    expect(body).not.toContain('Normal sleep is active');
  });

  it('disables every duration and labels the one starting', () => {
    awakeStore.loadPhase = 'ready';
    awakeStore.pending = { kind: 'manual', durationSecs: 3600 };
    const body = render(AwakeView).body;
    expect(body).toContain('Starting…');
    for (const label of ['30 Minutes', '1 Hour', '2 Hours', 'Indefinite']) {
      expect(isDisabled(buttonTag(body, label)), label).toBe(true);
    }
    expect(buttonTag(body, '1 Hour')).toContain('aria-busy="true"');
    expect(buttonTag(body, '30 Minutes')).toContain('aria-busy="false"');
  });

  it('locks rule switches until settings load and while a change saves', () => {
    awakeStore.loadPhase = 'ready';
    settingsStore.hasLoaded = false;
    const unloaded = render(AwakeView).body;
    const ruleSwitches = (body: string) => inputTags(body, /^Toggle .* rule$/);
    expect(ruleSwitches(unloaded).length).toBeGreaterThan(0);
    expect(ruleSwitches(unloaded).every(isDisabled)).toBe(true);

    settingsStore.hasLoaded = true;
    awakeStore.pending = null;
    const idle = render(AwakeView).body;
    expect(ruleSwitches(idle).some(isDisabled)).toBe(false);

    awakeStore.pending = { kind: 'rule', ruleId: 'rule.codex', action: 'delete' };
    const deleting = render(AwakeView).body;
    expect(ruleSwitches(deleting).every(isDisabled)).toBe(true);
    expect(deleting).toContain('Deleting ');
  });
});
