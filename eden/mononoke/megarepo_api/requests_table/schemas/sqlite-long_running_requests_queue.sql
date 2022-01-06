/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

 CREATE TABLE IF NOT EXISTS long_running_request_queue (
  `id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
  `repo_id` INTEGER NOT NULL,
  `bookmark` VARCHAR(512) NOT NULL,
  `request_type` varchar(255) NOT NULL,
  `args_blobstore_key` varchar(255) NOT NULL,
  `result_blobstore_key` varchar(255) DEFAULT NULL,
  `created_at` bigint(20) NOT NULL,
  `started_processing_at` bigint(20) DEFAULT NULL,
  `inprogress_last_updated_at` bigint(20) DEFAULT NULL,
  `ready_at` bigint(20) DEFAULT NULL,
  `polled_at` bigint(20) DEFAULT NULL,
  `status` VARCHAR(32) NOT NULL, -- enum('new','inprogress','ready','polled') NOT NULL DEFAULT 'new',
  `claimed_by` VARCHAR(255) NULL
);

CREATE INDEX IF NOT EXISTS `request_status` ON long_running_request_queue (`status`, `request_type`);
CREATE INDEX IF NOT EXISTS `request_creation` ON long_running_request_queue (`created_at`);
CREATE INDEX IF NOT EXISTS `request_dequeue` ON long_running_request_queue (`status`, `repo_id`, `created_at`);
CREATE INDEX IF NOT EXISTS `abandoned_request_index` ON long_running_request_queue (`repo_id`, `status`, `inprogress_last_updated_at`);
