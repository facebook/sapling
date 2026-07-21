/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

-- Per-manifest-repo tailer watermark. Tracks the last processed log id for each
-- manifest repo so the tailer can resume where it left off. Keyed by the
-- manifest repo's `repo_id`.
CREATE TABLE IF NOT EXISTS `manifest_watermark` (
  `repo_id` INTEGER PRIMARY KEY NOT NULL,
  `log_id` BIGINT NOT NULL
);
