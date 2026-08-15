export type StatusKind = 'ok' | 'error';

export interface StatusPresenter {
  show(message: string, kind: StatusKind): void;
  showSuccess(message: string): void;
  showError(message: string): void;
}

export function createStatusPresenter(
  element: HTMLElement,
): StatusPresenter {
  function show(message: string, kind: StatusKind): void {
    // Preserve the visible behavior while avoiding redundant DOM mutation.
    if (
      element.textContent === message
      && element.dataset.kind === kind
    ) {
      return;
    }

    element.textContent = message;
    element.dataset.kind = kind;
  }

  return {
    show,

    showSuccess(message) {
      show(message, 'ok');
    },

    showError(message) {
      show(message, 'error');
    },
  };
}