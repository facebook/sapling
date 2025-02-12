/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import os from 'node:os';
import * as vscode from 'vscode';

export const PERSISTED_STORAGE_KEY_PREFIX = 'isl-persisted:';

/**
 * Determine which command to use for `sl`, based on vscode configuration.
 * Changes to this setting require restarting, so it's ok to cache this value
 * or use it in the construction of a different object.
 */
export function getCLICommand(): string {
  // prettier-disable
  return (
    vscode.workspace.getConfiguration('sapling').get('commandPath') ||
    (os.platform() === 'win32' ? 'sl.exe' : 'sl')
  );
}

/** Whether the user has configured for files, diffs, and comparisons to open in ViewColumn.Beside instead of ViewColumn.Active. */
export function shouldOpenBeside(): boolean {
  return vscode.workspace.getConfiguration('sapling').get<boolean>('isl.openBeside') === true;
}
