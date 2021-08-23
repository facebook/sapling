/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

CREATE TABLE `ephemeral_bubbles` (
  `id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
  `created_at` BIGINT NOT NULL,
  `expires_at` BIGINT NOT NULL,
  `expired` BIT NOT NULL DEFAULT '0',
  `owner_identity` VARCHAR(255)
);

CREATE INDEX `ephemeral_bubbles_expires`
  ON `ephemeral_bubbles` (`expires_at`, `id`);

CREATE TABLE `ephemeral_bubble_changeset_mapping` (
    `repo_id` INT NOT NULL,
    `cs_id` BINARY(32) NOT NULL,
    `bubble_id` BIGINT NOT NULL,
    `gen` BIGINT NOT NULL,
    PRIMARY KEY (`repo_id`, `bubble_id`, `cs_id`)
)
