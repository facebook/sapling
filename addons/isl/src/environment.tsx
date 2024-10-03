/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/**
 * Returns true if running inside a vscode webview.
 * Could also be checked via the ISL Platform, but
 * that's only valid in ISL, and not more lightweight webviews
 * like the inline comments. This check should be valid with no additional setup.
 */
export function isVscode() {
  return window.location.protocol === 'vscode-webview:';
}
