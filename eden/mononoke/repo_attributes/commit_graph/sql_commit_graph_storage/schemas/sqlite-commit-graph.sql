/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

CREATE TABLE IF NOT EXISTS `commit_graph_edges` (
  `id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
  `repo_id` INTEGER NOT NULL,
  `cs_id` VARBINARY(32) NOT NULL,
  `gen` BIGINT NOT NULL,
  `subtree_source_gen` BIGINT NULL, -- NULL if same as gen
  `skip_tree_depth` BIGINT NOT NULL,
  `p1_linear_depth` BIGINT NOT NULL,
  `subtree_source_depth` BIGINT NULL, -- NULL if same as skip_tree_depth
  `parent_count` INTEGER NOT NULL,
  `subtree_source_count` INTEGER NOT NULL,
  `p1_parent` INTEGER NULL,
  `merge_ancestor` INTEGER NULL,
  `skip_tree_parent` INTEGER NULL,
  `skip_tree_skew_ancestor` INTEGER NULL,
  `p1_linear_skew_ancestor` INTEGER NULL,
  `subtree_or_merge_ancestor` INTEGER NULL, -- NULL if same as merge_ancestor
  `subtree_source_parent` INTEGER NULL, -- NULL if same as skip_tree_parent
  `subtree_source_skew_ancestor` INTEGER NULL, -- NULL if same as skip_tree_skew_ancestor
  UNIQUE (`repo_id`, `cs_id`)
);

CREATE TABLE IF NOT EXISTS `commit_graph_merge_parents` (
  `id` INTEGER NOT NULL,
  `parent_num` INTEGER NOT NULL,
  `parent` INTEGER NOT NULL,
  `repo_id` INTEGER NOT NULL,
  PRIMARY KEY (`id`, `parent_num`)
);

CREATE TABLE IF NOT EXISTS `commit_graph_subtree_sources` (
  `id` INTEGER NOT NULL,
  `subtree_source_num` INTEGER NOT NULL,
  `subtree_source` INTEGER NOT NULL,
  `repo_id` INTEGER NOT NULL,
  PRIMARY KEY (`id`, `subtree_source_num`)
);
