/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

CREATE TABLE IF NOT EXISTS `data` (
  `id` VARCHAR(255) NOT NULL,
  `creation_time` BIGINT NOT NULL,
  `chunk_id` VARCHAR(255) NOT NULL,
  `chunk_count` INT UNSIGNED NOT NULL,
  `chunking_method` INT UNSIGNED NOT NULL,
  PRIMARY KEY (`id`)
);

CREATE TABLE IF NOT EXISTS `chunk` (
  `id` VARCHAR(255) NOT NULL,
  `creation_time` TIMESTAMP DEFAULT CURRENT NOT NULL,
  `chunk_num` INT UNSIGNED NOT NULL,
  `value` BLOB NOT NULL,
  PRIMARY KEY (`id`, `chunk_num`)
);

CREATE TABLE IF NOT EXISTS `chunk_generation` (
    `id` VARCHAR(255) NOT NULL,
    `last_seen_generation` BIGINT UNSIGNED NOT NULL,
    `value_len` INT UNSIGNED NOT NULL,
    PRIMARY KEY (`id`)
);
