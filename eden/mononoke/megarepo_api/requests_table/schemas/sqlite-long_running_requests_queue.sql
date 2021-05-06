/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

 CREATE TABLE long_running_request_queue (
  `id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
  `request_type` varchar(255) NOT NULL,
  `args_blobstore_key` varchar(255) NOT NULL,
  `result_blobstore_key` varchar(255) DEFAULT NULL,
  `created_at` bigint(20) NOT NULL DEFAULT '0',
  `started_processing_at` bigint(20) DEFAULT NULL,
  `ready_at` bigint(20) DEFAULT NULL,
  `polled_at` bigint(20) DEFAULT NULL,
  `status` VARCHAR(32) NOT NULL -- enum('new','in_progress','ready','polled') NOT NULL DEFAULT 'new',
);
