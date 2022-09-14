/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

CREATE TABLE IF NOT EXISTS commit_graph_edges (
  id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
  repo_id INTEGER NOT NULL,
  cs_id VARBINARY(32) NOT NULL,
  gen BIGINT NOT NULL,
  skip_tree_depth BIGINT NOT NULL,
  p1_linear_depth BIGINT NOT NULL,
  parent_count INTEGER NOT NULL,
  p1_parent INTEGER NULL,
  merge_ancestor INTEGER NULL,
  skip_tree_parent INTEGER NULL,
  skip_tree_skew_ancestor INTEGER NULL,
  p1_linear_skew_ancestor INTEGER NULL,
  UNIQUE (repo_id, cs_id)
);

CREATE TABLE IF NOT EXISTS commit_graph_merge_parents (
  id INTEGER NOT NULL,
  parent_num INTEGER NOT NULL,
  parent INTEGER NOT NULL,
  PRIMARY KEY (id, parent_num)
);
