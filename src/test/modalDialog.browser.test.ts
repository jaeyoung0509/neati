/** @vitest-environment jsdom */
import { afterEach, describe, expect, it, vi } from 'vitest';
import { modalDialog } from '../lib/utils/modalDialog';

afterEach(() => { document.body.replaceChildren(); });

function fixture() {
  const main = document.createElement('main');
  const trigger = document.createElement('button');
  const fallback = document.createElement('button');
  const dialog = document.createElement('dialog');
  dialog.tabIndex = -1;
  dialog.innerHTML = '<h2 tabindex="-1">Review</h2><button id="cancel">Cancel</button><button id="confirm">Confirm</button>';
  // jsdom has no native modal isolation. Real browser evidence exercises that
  // separately; these stubs test the production action's lifecycle and keys.
  dialog.showModal = vi.fn(() => { dialog.open = true; });
  dialog.close = vi.fn(() => { dialog.open = false; });
  main.append(trigger, fallback, dialog);
  document.body.append(main);
  trigger.focus();
  return { main, trigger, fallback, dialog };
}

function tab(element: HTMLElement, shiftKey = false) {
  const event = new KeyboardEvent('keydown', { key: 'Tab', shiftKey, bubbles: true, cancelable: true });
  element.dispatchEvent(event);
  return event;
}

describe('modal workflow keyboard and lifetime', () => {
  it('starts at the safe action, contains both Tab directions and returns to the trigger', () => {
    const { trigger, dialog } = fixture();
    const action = modalDialog(dialog, { onCancel: vi.fn(), initialFocus: '#cancel' });
    const cancel = dialog.querySelector<HTMLButtonElement>('#cancel')!;
    const confirm = dialog.querySelector<HTMLButtonElement>('#confirm')!;
    expect(dialog.showModal).toHaveBeenCalledOnce();
    expect(document.activeElement).toBe(cancel);
    expect(tab(cancel, true).defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(confirm);
    expect(tab(confirm).defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(cancel);
    action.destroy();
    expect(dialog.close).toHaveBeenCalledOnce();
    expect(document.activeElement).toBe(trigger);
  });

  it('starts a long form at its title and skips closed disclosure, disabled and hidden controls', () => {
    const { dialog } = fixture();
    dialog.insertAdjacentHTML('beforeend', '<details><summary>Details</summary><button>Hidden detail</button></details><input type="hidden"><button disabled>Busy</button><button style="display:none">Hidden</button>');
    const action = modalDialog(dialog, { onCancel: vi.fn(), initialFocus: 'h2' });
    const title = dialog.querySelector('h2')!;
    const summary = dialog.querySelector('summary')!;
    expect(document.activeElement).toBe(title);
    tab(title, true);
    expect(document.activeElement).toBe(summary);
    tab(summary);
    expect(document.activeElement).toBe(dialog.querySelector('#cancel'));
    action.destroy();
  });

  it('keeps cancellation owned by the current busy workflow without duplicate listeners', () => {
    const { dialog } = fixture();
    const idle = vi.fn();
    const busy = vi.fn();
    const action = modalDialog(dialog, { onCancel: idle });
    action.update({ onCancel: busy });
    const event = new Event('cancel', { cancelable: true });
    dialog.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(true);
    expect(idle).not.toHaveBeenCalled();
    expect(busy).toHaveBeenCalledOnce();
    expect(dialog.open).toBe(true);
    action.destroy();
    dialog.dispatchEvent(new Event('cancel', { cancelable: true }));
    expect(busy).toHaveBeenCalledOnce();
  });

  it('uses the current explicit trigger after an asynchronous second-step dialog', () => {
    const { trigger, fallback, dialog } = fixture();
    const action = modalDialog(dialog, { onCancel: vi.fn(), returnFocusTarget: () => fallback });
    trigger.remove();
    action.destroy();
    expect(document.activeElement).toBe(fallback);
  });

  it('finds a reachable view action when the old trigger and the first candidate are hidden', () => {
    const { trigger, fallback, dialog } = fixture();
    const action = modalDialog(dialog, { onCancel: vi.fn() });
    trigger.hidden = true;
    action.destroy();
    expect(document.activeElement).toBe(fallback);
  });

  it('contains focus while all actions are disabled', () => {
    const { dialog } = fixture();
    const action = modalDialog(dialog, { onCancel: vi.fn(), initialFocus: 'h2' });
    for (const button of dialog.querySelectorAll('button')) button.disabled = true;
    expect(tab(dialog.querySelector('h2')!).defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(dialog);
    action.destroy();
  });
});
