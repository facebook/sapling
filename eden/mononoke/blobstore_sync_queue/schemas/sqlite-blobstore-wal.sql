/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

CREATE TABLE IF NOT EXISTS `blobstore_write_ahead_log` (
  `id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
  `blobstore_key` varchar NOT NULL,
  `timestamp` BIGINT NOT NULL, /* time the blob was added to the queue */
  `multiplex_id` INTEGER NOT NULL,
  `blob_size` BIGINT NOT NULL,
  `retry_count` INTEGER NOT NULL
);
