/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

CREATE TABLE `repo_lock` (
  `repo` VARCHAR(255) PRIMARY KEY,
  `state` INTEGER NOT NULL,
  `reason` VARCHAR(255)
);
