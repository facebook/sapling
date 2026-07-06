/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

CREATE TABLE IF NOT EXISTS enabled_derived_data_types (
  repo_id           INTEGER NOT NULL,
  derived_data_type VARCHAR(255) NOT NULL,
  root_request_id   INT DEFAULT NULL,
  PRIMARY KEY (repo_id, derived_data_type)
);

-- Lookups by `repo_id` ("types enabled for a repo") are served by the
-- leftmost prefix of the primary key. This secondary index serves the reverse
-- lookup ("repos with a given type enabled"); ordering `repo_id` after
-- `derived_data_type` makes that an index-only scan.
CREATE INDEX IF NOT EXISTS derived_data_type_repo_id_idx
  ON enabled_derived_data_types (derived_data_type, repo_id);

-- "Repos enabled by a given campaign" (WHERE root_request_id = ?); `repo_id`
-- last makes it an index-only scan.
CREATE INDEX IF NOT EXISTS root_request_id_repo_id_idx
  ON enabled_derived_data_types (root_request_id, repo_id);
