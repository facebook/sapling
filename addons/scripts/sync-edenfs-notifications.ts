/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Internal} from './Internal';

/* eslint-disable no-console */

/**
 * This script is run with `yarn sync-edenfs-notifications` in addons/ to sync the EdenFS notifications client files.
 * This is a no-op in OSS.
 */
async function main() {
  await Internal.syncEdenFSNotificationsClient?.();
}

main().catch(error => {
  console.error(error);
  process.exit(1);
});
