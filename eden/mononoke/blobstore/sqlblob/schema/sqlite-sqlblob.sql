/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

CREATE TABLE `data` (
  `id` VARCHAR(255) NOT NULL,
  `type` TINYINT NOT NULL,
  `value` BLOB NOT NULL,
  PRIMARY KEY (`id`)
);

CREATE TABLE `chunk` (
  `id` VARCHAR(255) NOT NULL,
  `chunk_id` INT UNSIGNED NOT NULL,
  `value` BLOB NOT NULL,
  PRIMARY KEY (`id`, `chunk_id`)
);
