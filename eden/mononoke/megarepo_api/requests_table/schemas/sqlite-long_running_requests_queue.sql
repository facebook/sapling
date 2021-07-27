/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

 CREATE TABLE long_running_request_queue (
  `id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
  `repo_id` INTEGER NOT NULL,
  `bookmark` VARCHAR(512) NOT NULL,
  `request_type` varchar(255) NOT NULL,
  `args_blobstore_key` varchar(255) NOT NULL,
  `result_blobstore_key` varchar(255) DEFAULT NULL,
  `created_at` bigint(20) NOT NULL,
  `started_processing_at` bigint(20) DEFAULT NULL,
  `ready_at` bigint(20) DEFAULT NULL,
  `polled_at` bigint(20) DEFAULT NULL,
  `status` VARCHAR(32) NOT NULL, -- enum('new','in_progress','ready','polled') NOT NULL DEFAULT 'new',
  `claimed_by` VARCHAR(255) NULL
);

CREATE INDEX `request_status` ON long_running_request_queue (`status`, `request_type`);
CREATE INDEX `request_creation` ON long_running_request_queue (`created_at`);
CREATE INDEX `request_dequeue` ON long_running_request_queue (`status`, `repo_id`, `created_at`);
