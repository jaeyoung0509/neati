import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import AwakeRuleEditor from '../lib/components/AwakeRuleEditor.svelte';
import type { AwakeRule } from '../lib/models/types';

const legacyRule: AwakeRule = {
  id: 'rule.custom',
  app_name: 'Nightly build',
  executable_pattern: 'cargo',
  requires_process_pattern: 'rustc',
  application: null,
  agent_ids: [],
  behavior: 'keep_display_awake',
  power_condition: 'always',
  enabled: true,
};

describe('AwakeRuleEditor', () => {
  it('opens an empty app rule that cannot be saved until an application is chosen', () => {
    const { body } = render(AwakeRuleEditor, { props: { mode: 'basic', rule: null, onClose: () => undefined } });
    expect(body).toContain('Build an app rule');
    expect(body).toContain('Choose from installed applications');
    expect(body).toMatch(/<button[^>]*disabled[^>]*>(?:<!--[^>]*-->)*Save rule/);
  });

  it('seeds a legacy rule draft from the rule it was opened with', () => {
    const { body } = render(AwakeRuleEditor, { props: { mode: 'advanced', rule: legacyRule, onClose: () => undefined } });
    expect(body).toContain('Edit legacy process rule');
    expect(body).toContain('value="Nightly build"');
    expect(body).toContain('value="cargo"');
    expect(body).toContain('value="rustc"');
    expect(body).toContain('Save changes');
  });
});
