/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Internal} from './Internal';

/* eslint-disable no-console */

/**
 * This script is run with `yarn sync-api` in addons/ to sync the VS Code API types elsewhere in the repo.
 * This is a no-op in OSS. */
async function main() {
  await Internal.syncSaplingVSCodeAPITypes?.();
}

main().catch(error => {
  console.error(error);
  process.exit(1);
});
