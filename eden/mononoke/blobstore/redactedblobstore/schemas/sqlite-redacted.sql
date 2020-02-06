/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

CREATE TABLE  `censored_contents` (
	`id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
	`content_key` VARCHAR(255) NOT NULL,
	`task` VARCHAR(64) NOT NULL,
	`add_timestamp` BIGINT(20) NOT NULL
);

CREATE INDEX `content_key`
ON `censored_contents` (`content_key`);
