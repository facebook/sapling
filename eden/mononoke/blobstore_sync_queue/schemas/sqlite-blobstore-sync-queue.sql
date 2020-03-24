/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

CREATE TABLE `blobstore_sync_queue` (
  `id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
  `blobstore_key` varchar NOT NULL,
  `blobstore_id` INTEGER NOT NULL,
  `add_timestamp` BIGINT NOT NULL,
  `multiplex_id` INTEGER NOT NULL,
  `original_timestamp` BIGINT NOT NULL DEFAULT 0,
  `operation_key` BINARY(16) NOT NULL DEFAULT X'00000000000000000000000000000000'
);
