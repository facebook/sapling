/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {isWindows, isMac} from 'shared/OperatingSystem';

// https://github.com/microsoft/vscode/blob/c90951b/src/vs/workbench/browser/style.ts#L72
const fontFamily = isWindows
  ? '"Segoe WPC", "Segoe UI", sans-serif'
  : isMac
  ? '-apple-system, BlinkMacSystemFont, sans-serif'
  : 'system-ui, "Ubuntu", "Droid Sans", sans-serif';

// https://github.com/microsoft/vscode/blob/c90951b147164bb427d08f2c251666d5610076d3/src/vs/workbench/contrib/webview/browser/themeing.ts#L68-L76
const fontSize = '13px';

/**
 * Default "reset" CSS to normalize things like font size, etc.
 * This is intended to match VSCode webview's CSS and should be skipped
 * when running inside VSCode (by setting theme.resetCSS to '').
 */
// https://github.com/microsoft/vscode/blob/c90951b147164bb427d08f2c251666d5610076d3/src/vs/workbench/contrib/webview/browser/pre/index.html#L96-L98
export const DEFAULT_RESET_CSS = `
html {
  --vscode-font-size: ${fontSize};
  --vscode-font-family: ${fontFamily};
}

body {
  font-family: var(--vscode-font-family);
  font-size: var(--vscode-font-size);
}
`;
