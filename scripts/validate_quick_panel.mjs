import { execFileSync } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

// Build first, then serve /private/tmp/neati-quick-panel-validation on localhost.
const url = process.argv[2] ?? 'http://127.0.0.1:5187/src/test/fixtures/quick-panel.html';
const output = resolve(process.argv[3] ?? '/private/tmp/neati-quick-panel-evidence');
const session = 'neati-quick-panel-validation';
mkdirSync(output, { recursive: true });
function browser(...args) {
  return execFileSync('agent-browser', ['--session', session, ...args], { encoding: 'utf8', timeout: 60_000 });
}
function evaluate(source) {
  const result = JSON.parse(browser('eval', source, '--json'));
  if (!result.success) throw new Error(JSON.stringify(result.error));
  return result.data.result;
}
try {
  browser('open', url);
  browser('wait', '--fn', 'window.quickPanelValidation?.ready === true');
  const evidence = evaluate(`(async () => {
    const driver = window.quickPanelValidation;
    const assert = (condition, message) => { if (!condition) throw new Error(message); };
    const states = [{ state: 'loading', ...driver.measurement() }];
    for (const state of ['partial', 'fresh', 'stale', 'error', 'timeout', 'disconnected', 'empty', 'long', 'fresh']) {
      states.push({ state, ...await driver.transition(state) });
    }
    assert(states.every(row => row.width === 360 && row.height === states[0].height && row.footerBottom === states[0].footerBottom && row.gaugeSlots === 2 && row.requests.length === 1), JSON.stringify(states));
    assert(states.find(row => row.state === 'fresh').meters === 2, 'Fresh usage lost its meters');
    assert(states.find(row => row.state === 'long').scrollable, 'Long status fixture did not exercise body overflow');
    assert(states.filter(row => ['loading', 'stale', 'error', 'timeout', 'disconnected', 'empty'].includes(row.state)).every(row => row.meters === 0), 'Unknown usage rendered a meter');
    const hidden = await driver.visibility(false);
    assert(hidden.subscribers === 0, 'Hidden panel retained its usage subscription');
    const reopened = await driver.visibility(true);
    assert(reopened.requests.length === 1 && reopened.subscribers === 1, 'Reopening resized or duplicated subscriptions');
    const delayed = await driver.delayedMonitor();
    assert(delayed.before === delayed.after, 'Obsolete monitor result resized a newer activation');
    const widths = [];
    for (const width of [320, 400, 360]) {
      const before = driver.measurement().requests.length;
      const row = await driver.viewport(width);
      assert(row.width === width && row.requests.length <= before + 1, 'Width resize feedback loop');
      const refreshed = await driver.transition('fresh');
      assert(row.height === refreshed.height && row.requests.length === refreshed.requests.length, 'Telemetry resized a fitted width');
      widths.push(row);
    }
    const small = await driver.viewport(320, 360);
    assert(small.height === 360 && small.scrollable && small.footerBottom <= 360, 'Constrained display lost scrolling/footer');
    const pref = await driver.preferences();
    assert(pref.height <= 360 && pref.requests.length <= small.requests.length + 1, 'Preference change repeated resize');
    return { states, hidden, reopened, delayed, widths, small, pref };
  })()`);
  writeFileSync(resolve(output, 'mounted-transitions.json'), `${JSON.stringify(evidence, null, 2)}\n`);
  // Fresh mounts keep the screenshots on the complete saved section layout.
  for (const width of [320, 360, 400]) {
    for (const theme of ['light', 'dark']) {
      browser('set', 'viewport', String(width), '800');
      browser('open', url);
      browser('wait', '--fn', 'window.quickPanelValidation?.ready === true');
      const measured = evaluate(`(async () => { document.documentElement.classList.toggle('dark', ${theme === 'dark'}); await window.quickPanelValidation.viewport(${width}); return await window.quickPanelValidation.transition('fresh'); })()`);
      browser('set', 'viewport', String(width), String(measured.height));
      browser('screenshot', resolve(output, `quick-${width}-${theme}.png`));
    }
  }
  browser('set', 'media', 'dark', 'reduced-motion');
  const reduced = evaluate(`(async () => {
    const driver = window.quickPanelValidation;
    const before = driver.measurement();
    await driver.transition('loading');
    const after = await driver.transition('fresh');
    if (!matchMedia('(prefers-reduced-motion: reduce)').matches || before.height !== after.height || before.requests.length !== after.requests.length) throw new Error('Reduced-motion transition changed bounds');
    return after;
  })()`);
  writeFileSync(resolve(output, 'reduced-motion.json'), `${JSON.stringify(reduced, null, 2)}\n`);
  evaluate(`(async () => { await window.quickPanelValidation.viewport(320, 360); return window.quickPanelValidation.measurement(); })()`);
  browser('set', 'viewport', '320', '360');
  browser('screenshot', resolve(output, 'quick-constrained.png'));
  browser('press', 'Escape');
  const closed = evaluate('window.quickPanelValidation.measurement()');
  if (closed.visible || closed.subscribers !== 0) throw new Error('Escape failed to deactivate the panel');
  writeFileSync(resolve(output, 'keyboard-close.json'), `${JSON.stringify(closed, null, 2)}\n`);
  console.log(`Mounted Quick Panel transition checks passed. Evidence: ${output}`);
} finally {
  browser('close');
}
