/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

CREATE TABLE IF NOT EXISTS `revision_references` (
  `repo` VARBINARY(64) NOT NULL,
  `namespace` VARBINARY(32) NOT NULL,
  `name` VARBINARY(256) NULL,
  `value` VARBINARY(40) NOT NULL
);
