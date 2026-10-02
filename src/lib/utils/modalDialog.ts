import { isFocusable, restoreFocus } from './focus';

interface ModalOptions {
  onCancel: () => void;
  initialFocus?: string;
  returnFocusTarget?: () => HTMLElement | null;
}

/** Keep native modal Tab cycling in visible controls instead of the document. */
export function trapDialogFocus(dialog: HTMLDialogElement) {
  const handleKeydown = (event: KeyboardEvent) => {
    if (event.key !== 'Tab') return;
    const controls = [...dialog.querySelectorAll<HTMLElement>(
      'button, a[href], input, select, textarea, summary, iframe, [contenteditable="true"], [tabindex]',
    )].filter((element) => element.tabIndex >= 0 && isFocusable(element));
    const first = controls[0];
    const last = controls[controls.length - 1];
    const active = document.activeElement;
    if (!first || !last) {
      event.preventDefault();
      dialog.focus({ preventScroll: true });
    } else if (!controls.includes(active as HTMLElement)) {
      event.preventDefault();
      (event.shiftKey ? last : first).focus();
    } else if (event.shiftKey && active === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && active === last) {
      event.preventDefault();
      first.focus();
    }
  };

  dialog.addEventListener('keydown', handleKeydown);
  return { destroy() { dialog.removeEventListener('keydown', handleKeydown); } };
}

/** Native modal isolation with one keyboard and focus-return contract. */
export function modalDialog(dialog: HTMLDialogElement, initialOptions: ModalOptions) {
  let options = initialOptions;
  const previousFocus = document.activeElement instanceof HTMLElement
    ? document.activeElement
    : null;
  const fallbackContainer = dialog.closest('main');
  const handleCancel = (event: Event) => {
    // The owning workflow decides whether cancellation is allowed while busy.
    event.preventDefault();
    options.onCancel();
  };
  const focusTrap = trapDialogFocus(dialog);
  dialog.addEventListener('cancel', handleCancel);
  dialog.showModal();
  const initialTarget = options.initialFocus
    ? dialog.querySelector<HTMLElement>(options.initialFocus)
    : null;
  (initialTarget ?? dialog).focus({ preventScroll: true });
  dialog.scrollTop = 0;

  return {
    update(nextOptions: ModalOptions) { options = nextOptions; },
    destroy() {
      dialog.removeEventListener('cancel', handleCancel);
      focusTrap.destroy();
      if (dialog.open) dialog.close();
      restoreFocus(options.returnFocusTarget?.() ?? previousFocus, fallbackContainer);
    },
  };
}
