/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

// Detect uncaught JavaScript errors and write them to the DOM in dev mode
if (import.meta.hot) {
  let errorContainer: HTMLDivElement | null = null;

  const createErrorContainer = () => {
    if (!errorContainer) {
      errorContainer = document.createElement('div');
      errorContainer.id = 'uncaught-error-display';
      errorContainer.style.cssText = `
        position: fixed;
        bottom: 0;
        left: 0;
        right: 0;
        max-height: 300px;
        background-color: rgba(220, 38, 38, 0.95);
        color: white;
        font-family: monospace;
        font-size: 12px;
        z-index: 999999;
        border-top: 3px solid #991b1b;
        display: flex;
        flex-direction: column;
      `;

      // Create header row
      const header = document.createElement('div');
      header.style.cssText = `
        display: flex;
        justify-content: space-between;
        align-items: center;
        padding: 12px 16px;
        border-bottom: 1px solid rgba(255, 255, 255, 0.3);
        font-weight: bold;
      `;

      const title = document.createElement('span');
      title.textContent = 'Uncaught Webview Errors (development mode only)';
      header.appendChild(title);

      const closeButton = document.createElement('button');
      closeButton.textContent = '×';
      closeButton.style.cssText = `
        background: none;
        border: none;
        color: white;
        font-size: 24px;
        line-height: 1;
        cursor: pointer;
        padding: 0;
        width: 24px;
        height: 24px;
        display: flex;
        align-items: center;
        justify-content: center;
      `;
      closeButton.onclick = () => {
        if (errorContainer) {
          errorContainer.style.display = 'none';
        }
      };
      header.appendChild(closeButton);

      // Create errors content container
      const errorsContent = document.createElement('div');
      errorsContent.id = 'uncaught-error-display-content';
      errorsContent.style.cssText = `
        overflow-y: auto;
        padding: 16px;
        flex: 1;
      `;

      errorContainer.appendChild(header);
      errorContainer.appendChild(errorsContent);
      document.body.appendChild(errorContainer);
    }
    return errorContainer;
  };

  const displayError = (
    message: string,
    source?: string,
    lineno?: number,
    colno?: number,
    error?: Error,
  ) => {
    const container = createErrorContainer();
    const errorsContent = container.querySelector('#uncaught-error-display-content');
    if (!errorsContent) {
      return;
    }

    // Show the container if it was hidden
    container.style.display = 'flex';

    const errorEntry = document.createElement('div');
    errorEntry.style.cssText = `
      margin-bottom: 12px;
      padding-bottom: 12px;
      border-bottom: 1px solid rgba(255, 255, 255, 0.3);
    `;

    const timestamp = new Date().toLocaleTimeString();

    // Create header line
    const header = document.createElement('strong');
    header.textContent = `[${timestamp}] Uncaught Error:`;
    errorEntry.appendChild(header);
    errorEntry.appendChild(document.createElement('br'));

    // Add error message
    const messageText = document.createTextNode(message);
    errorEntry.appendChild(messageText);
    errorEntry.appendChild(document.createElement('br'));

    // Add source location if available
    if (source) {
      const sourceSpan = document.createElement('span');
      sourceSpan.style.opacity = '0.8';
      sourceSpan.textContent = `at ${source}:${lineno}:${colno}`;
      errorEntry.appendChild(sourceSpan);
      errorEntry.appendChild(document.createElement('br'));
    }

    // Add stack trace if available
    if (error?.stack) {
      const stackPre = document.createElement('pre');
      stackPre.style.cssText =
        'margin-top: 8px; opacity: 0.9; white-space: pre-wrap; word-break: break-all;';
      stackPre.textContent = error.stack;
      errorEntry.appendChild(stackPre);
    }

    errorsContent.insertBefore(errorEntry, errorsContent.firstChild);
  };

  const handleError = (event: ErrorEvent) => {
    displayError(event.message, event.filename, event.lineno, event.colno, event.error);
  };

  const handleUnhandledRejection = (event: PromiseRejectionEvent) => {
    const error = event.reason;
    let message = 'Unhandled Promise Rejection';
    let errorObj: Error | undefined;

    if (error instanceof Error) {
      message = error.message;
      errorObj = error;
    } else if (typeof error === 'string') {
      message = error;
    } else if (error != null) {
      message = String(error);
    }

    displayError(`Promise: ${message}`, undefined, undefined, undefined, errorObj);
  };

  window.addEventListener('error', handleError);
  window.addEventListener('unhandledrejection', handleUnhandledRejection);

  // TODO: we should probably call this on hot reload somehow
  const _dispose = () => {
    window.removeEventListener('error', handleError);
    window.removeEventListener('unhandledrejection', handleUnhandledRejection);
    errorContainer?.remove();
    errorContainer = null;
  };
}
