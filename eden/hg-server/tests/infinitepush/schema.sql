/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

CREATE TABLE `bookmarkstonode` (
  `node` varbinary(64) NOT NULL,
  `bookmark` varbinary(512) NOT NULL,
  `reponame` varbinary(255) NOT NULL,
  `time` datetime DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  PRIMARY KEY (`reponame`,`bookmark`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8;

CREATE TABLE `bundles` (
  `bundle` varbinary(512) NOT NULL,
  `reponame` varbinary(255) NOT NULL,
  PRIMARY KEY (`bundle`,`reponame`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8;

CREATE TABLE `nodestobundle` (
  `node` varbinary(64) NOT NULL,
  `bundle` varbinary(512) NOT NULL,
  `reponame` varbinary(255) NOT NULL,
  PRIMARY KEY (`node`,`reponame`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8;

CREATE TABLE `nodesmetadata` (
  `node` varbinary(64) NOT NULL,
  `message` mediumblob NOT NULL,
  `p1` varbinary(64) NOT NULL,
  `p2` varbinary(64) DEFAULT NULL,
  `author` varbinary(255) NOT NULL,
  `committer` varbinary(255) DEFAULT NULL,
  `author_date` bigint(20) NOT NULL,
  `committer_date` bigint(20) DEFAULT NULL,
  `reponame` varbinary(255) NOT NULL,
  `optional_json_metadata` mediumblob,
  PRIMARY KEY (`reponame`,`node`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8;

CREATE TABLE `forwardfillerqueue` (
  `id` BIGINT(20) UNSIGNED NOT NULL AUTO_INCREMENT,
  `bundle` VARBINARY(64) NOT NULL,
  `reponame` VARBINARY(255) NOT NULL,
  `slice` TINYINT(3) UNSIGNED DEFAULT 0,
  `created_at` DATETIME DEFAULT CURRENT_TIMESTAMP,
  `claimed_by` VARCHAR(255) NULL,
  `claimed_at` DATETIME NULL,
  PRIMARY KEY (`id`),
  KEY `queue_order` (`reponame`, `slice`, `id`),
  KEY `claim_review` (`claimed_at`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8;

CREATE TABLE `replaybookmarksqueue` (
  `id` BIGINT(20) UNSIGNED NOT NULL AUTO_INCREMENT,
  `reponame` varbinary(255) NOT NULL,
  `bookmark` varbinary(512) NOT NULL,
  `node` varbinary(64) NOT NULL,
  `bookmark_hash` varbinary(64) NOT NULL,
  `created_at` DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  `synced` TINYINT(1) NOT NULL DEFAULT 0,
  PRIMARY KEY (`id`),
  KEY `sync_queue` (`synced`, `reponame`, `bookmark_hash`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8
