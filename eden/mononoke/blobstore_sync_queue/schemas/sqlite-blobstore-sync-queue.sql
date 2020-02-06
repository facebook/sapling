/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

CREATE TABLE `blobstore_sync_queue` (
  `id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
  `blobstore_key` varchar NOT NULL,
  `blobstore_id` INTEGER NOT NULL,
  `add_timestamp` BIGINT NOT NULL
);
