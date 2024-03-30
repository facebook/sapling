/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

-- bookmarks
-- Contains the set of bookmarks for each workspace
CREATE TABLE IF NOT EXISTS  `workspace_bookmarks` (
  `reponame` VARCHAR(255) NOT NULL,
  `workspace` VARCHAR(255) NOT NULL,
  `name` VARCHAR(512) NOT NULL,
  `commit` VARBINARY(32) NOT NULL,
  PRIMARY KEY (`reponame`, `workspace`, `name`)
);


-- checkout locations
-- Contains the checkout locations
CREATE TABLE IF NOT EXISTS  `checkoutlocations` (
  `reponame` VARCHAR(255) NOT NULL,
  `workspace` VARCHAR(255) NOT NULL,
  `hostname` VARCHAR(255) NOT NULL,
  `commit` VARCHAR(255) NOT NULL,
  `checkout_path` VARCHAR(255) NOT NULL,
  `shared_path` VARCHAR(255) NOT NULL,
  `timestamp` DATETIME DEFAULT CURRENT_TIMESTAMP,
  `unixname` VARCHAR(255) NOT NULL,
  PRIMARY KEY (`reponame`,`workspace`,`hostname`,`checkout_path`)
);


-- heads
-- Contains the set of heads for each workspace
CREATE TABLE IF NOT EXISTS `heads` (
  `reponame` VARCHAR(255) NOT NULL,
  `workspace` VARCHAR(255) NOT NULL,
  `commit` VARBINARY(32) NOT NULL,
  `seq` INTEGER PRIMARY KEY AUTOINCREMENT,
  UNIQUE(`reponame`, `workspace`, `commit`)
);
CREATE INDEX `reponame_commit` ON `heads`(`reponame`, `commit`);
CREATE INDEX `seq` ON `heads`(`seq`);


-- history
-- Creates the table to store historical version of timeline
CREATE TABLE IF NOT EXISTS  `history`(
  `reponame` VARCHAR(255) NOT NULL,
  `workspace` VARCHAR(255) NOT NULL,
  `version` BIGINT(20) NOT NULL,
  `date` DATE NOT NULL,
  `timestamp` TIMESTAMP DEFAULT CURRENT NOT NULL,
  `heads` BLOB NOT NULL,
  `bookmarks` BLOB NOT NULL,
  `remotebookmarks` BLOB NOT NULL,
  PRIMARY KEY (`reponame`,`workspace`,`version`)
);
CREATE INDEX `reponame_workspace_date` ON `history`(`reponame`, `workspace`, `date`);
CREATE INDEX `reponame_workspace_timestamp` ON `history`(`reponame`, `workspace`, `timestamp`);


-- remotebookmarks
-- Contains the set of remotebookmarks for each workspace
CREATE TABLE IF NOT EXISTS  `remotebookmarks` (
  `reponame` VARCHAR(255) NOT NULL,
  `workspace` VARCHAR(255) NOT NULL,
  `remote` VARCHAR(255) NOT NULL,
  `name` VARCHAR(512) NOT NULL,
  `commit` VARBINARY(32) NOT NULL,
  PRIMARY KEY (`reponame`,`workspace`,`remote`,`name`)
);


-- snapshots
-- Contains the set of snapshots for each workspace
CREATE TABLE IF NOT EXISTS  `snapshots` (
  `reponame` VARCHAR(255) NOT NULL,
  `workspace` VARCHAR(255) NOT NULL,
  `commit` VARBINARY(32) NOT NULL,
  `seq` INTEGER PRIMARY KEY AUTOINCREMENT,
  UNIQUE(`reponame`, `workspace`, `commit`)
);
CREATE INDEX `seq` ON snapshots(`seq`);


-- versions
-- Contains the latest version number of a workspace.
CREATE TABLE IF NOT EXISTS  `versions` (
  `reponame` VARCHAR(255) NOT NULL,
  `workspace` VARCHAR(255) NOT NULL,
  `version` BIGINT(20) NOT NULL,
  `timestamp` TIMESTAMP  NULL,
  `archived` BOOLEAN DEFAULT FALSE,
  PRIMARY KEY (`reponame`,`workspace`)
);
