/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import os from 'os';
import * as vscode from 'vscode';

/**
 * Determine which command to use for `sl`, based on vscode configuration.
 * Changes to this setting require restarting, so it's ok to cache this value
 * or use it in the construction of a different object.
 */
export function getCLICommand(): string {
  // prettier-disable
  return (
    vscode.workspace.getConfiguration('sapling').get('commandPath') ||
    // @fb-only
    (os.platform() === 'win32' ? 'sl.exe' : 'sl')
  );
}
