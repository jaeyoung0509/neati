import { describeError } from '../utils/errors';
import type { AwakeBehavior, AwakeRule, AwakeState } from '../models/types';
import {
  tauriDisableManualAwake,
  tauriGetAwakeState,
  tauriSetManualAwake,
} from '../utils/tauri';
import { settingsStore } from './settings.svelte';
import { loadFailed, loadStarted, type LoadPhase } from '../utils/loadPhase';

/** The one Keep Awake change in flight; the state changes only after the backend confirms it. */
export type AwakeChange =
  | { kind: 'manual'; durationSecs: number | null }
  | { kind: 'stop-manual' }
  | { kind: 'rule'; ruleId: string; action: 'toggle' | 'delete' | 'save' }
  | { kind: 'recommended-rules' };

/** Rules the "Enable recommended AC-only rules" action turns on. */
export const RECOMMENDED_AWAKE_RULE_IDS: readonly string[] = ['rule.codex', 'rule.claude', 'rule.docker'];

export class AwakeStore {
  state = $state<AwakeState>({
    is_active: false,
    behavior: null,
    trigger_source: null,
    active_process_name: null,
    active_rule_id: null,
    manual_expires_at: null,
    active_rules_count: 0,
    power_source: 'unknown',
    last_error: null,
    rule_evaluations: [],
    evaluation_health: {
      status: 'healthy',
      last_completed_at: null,
      last_failed_at: null,
      last_failure_reason: null,
    },
  });
  /** Until the first state read answers, "Normal sleep is active" is not a result (#491). */
  loadPhase = $state<LoadPhase>('idle');
  pending = $state<AwakeChange | null>(null);
  /** Why the last manual change failed; cleared when the next one starts. */
  error = $state<string | null>(null);
  /** Why the latest state read failed; cleared by the next successful read. */
  readError = $state<string | null>(null);

  /** A Keep Awake change is being saved; every other change waits for it. */
  get isLoading(): boolean {
    return this.pending !== null;
  }

  async refresh() {
    this.loadPhase = loadStarted(this.loadPhase);
    try {
      this.state = await tauriGetAwakeState();
      this.loadPhase = 'ready';
      this.readError = null;
    } catch (e) {
      this.readError = describeError(e) || 'Keep Awake state could not be read.';
      this.loadPhase = loadFailed(this.loadPhase);
    }
  }

  /**
   * Runs one change at a time. A second request while one is saving is
   * ignored (`null`) instead of racing the first, so a double click cannot
   * save twice or apply an older rule list over a newer one.
   */
  private async change<T>(change: AwakeChange, work: () => Promise<T>): Promise<T | null> {
    if (this.pending) return null;
    this.pending = change;
    try {
      return await work();
    } finally {
      this.pending = null;
    }
  }

  private async saveRules(rules: AwakeRule[]): Promise<boolean> {
    const saved = await settingsStore.save({ awake_rules: rules as typeof settingsStore.settings.awake_rules });
    if (saved) await this.refresh();
    return saved;
  }

  async toggleRule(ruleId: string): Promise<boolean> {
    const saved = await this.change({ kind: 'rule', ruleId, action: 'toggle' }, () =>
      this.saveRules(settingsStore.settings.awake_rules.map((r) =>
        r.id === ruleId ? { ...r, enabled: !r.enabled } : r
      ))
    );
    return saved ?? false;
  }

  async addRule(rule: AwakeRule): Promise<boolean> {
    const saved = await this.change({ kind: 'rule', ruleId: rule.id, action: 'save' }, () =>
      this.saveRules([...settingsStore.settings.awake_rules, rule])
    );
    return saved ?? false;
  }

  async deleteRule(ruleId: string): Promise<boolean> {
    const saved = await this.change({ kind: 'rule', ruleId, action: 'delete' }, () =>
      this.saveRules(settingsStore.settings.awake_rules.filter((r) => r.id !== ruleId))
    );
    return saved ?? false;
  }

  async updateRule(rule: AwakeRule): Promise<boolean> {
    const saved = await this.change({ kind: 'rule', ruleId: rule.id, action: 'save' }, () =>
      this.saveRules(settingsStore.settings.awake_rules.map((r) => (r.id === rule.id ? rule : r)))
    );
    return saved ?? false;
  }

  /** Turns on the reviewed recommended rules, plugged in only. */
  async enableRecommendedRules(): Promise<boolean> {
    const saved = await this.change({ kind: 'recommended-rules' }, () =>
      this.saveRules(settingsStore.settings.awake_rules.map((rule) =>
        RECOMMENDED_AWAKE_RULE_IDS.includes(rule.id)
          ? { ...rule, enabled: true, power_condition: 'ac_power_only' as const }
          : rule
      ))
    );
    return saved ?? false;
  }

  async setManual(durationSecs: number | null, behavior: AwakeBehavior = 'prevent_system_sleep') {
    await this.change({ kind: 'manual', durationSecs }, async () => {
      this.error = null;
      try {
        await tauriSetManualAwake(durationSecs, behavior);
        await this.refresh();
      } catch (e) {
        this.error = describeError(e) || null;
        throw e;
      }
    });
  }

  async disableManual() {
    await this.change({ kind: 'stop-manual' }, async () => {
      this.error = null;
      try {
        await tauriDisableManualAwake();
        await this.refresh();
      } catch (e) {
        this.error = describeError(e) || null;
      }
    });
  }
}

export const awakeStore = new AwakeStore();
