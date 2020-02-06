/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
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
