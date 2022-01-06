/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

CREATE TABLE IF NOT EXISTS mutable_counters (
  repo_id INT UNSIGNED NOT NULL,
  name VARCHAR(128) NOT NULL,
  value BIGINT NOT NULL,
  PRIMARY KEY (repo_id, name)
);
