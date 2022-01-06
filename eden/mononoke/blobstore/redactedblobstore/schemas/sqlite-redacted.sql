/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

CREATE TABLE IF NOT EXISTS  `censored_contents` (
	`id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
	`content_key` VARCHAR(255) NOT NULL,
	`task` VARCHAR(64) NOT NULL,
	`add_timestamp` BIGINT(20) NOT NULL,
	`log_only` BIT DEFAULT NULL,
	UNIQUE(`content_key`)
);

CREATE INDEX IF NOT EXISTS `content_key`
ON `censored_contents` (`content_key`);
