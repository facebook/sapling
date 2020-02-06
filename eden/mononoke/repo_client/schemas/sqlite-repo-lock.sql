/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

CREATE TABLE `repo_lock` (
  `repo` VARCHAR(255) PRIMARY KEY,
  `state` INTEGER NOT NULL,
  `reason` VARCHAR(255)
);
