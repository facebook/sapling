/*
Mononoke data model

This is my attempt to put the entire data model into one place in a semi-formalized way.

Notes:
- I renamed `complete` to `changeset`, which seems a bit more descriptive.
- There's a tons of VARBINARY(32) joins between tables - giving changesets and unodes
  dedicated `id` fields makes references much smaller.
- I'm assuming text-like fields such as bookmark names and paths are UTF8
- Does SQL have a way to define type aliases? Because that would be useful here.
- This implements the N ordered parent model, for both changesets and unodes

Regarding unodes, it looks like we won't need separate filenode and filelog tables as they're
just indexes on the unode table. But maybe that's making the table too fat and it would be
better to separate out less used indexes? Similar tradeoff if unode has cs backlink, linknode
becomes just another index.

Do we want a generation number for unodes too? Is it necessary to make it possible to
order filelog in a sensible way?

I've done a first approximation of obsmarkers as an "action marker" - ie, something happened
which creates a relationship between two changesets, without necessarily implying
obsolescence or hiding.

I've put heads and bookmarks in separate tables. The main requirement is that they're
updated under the same transaction.

I added FOREIGN KEY constraints for documentation purposes. AOSP doesn't support them at all so
they won't be much use in implementation (also they won't work with shards).

References:
File and tree history storage - https://our.intern.facebook.com/intern/wiki/Source_Control/Mononoke/Design/File_and_Tree_Storage/
Re-envisioning file history - https://our.intern.facebook.com/intern/wiki/Source_Control/Mononoke/Design/Unodes/
Linknodes - https://our.intern.facebook.com/intern/wiki/Source_Control/Mononoke/Design/Linknodes/

*/

-- CREATE DATABASE `mononoke` CHARSET 'utf8' COLLATE 'utf8_bin';
-- USE `mononoke`;

-- PSEUDO? table representing config. Or should this be SOT?
-- SHARDING: nope
CREATE TABLE `repos` (
    `id` INT UNSIGNED NOT NULL UNIQUE AUTO_INCREMENT,
    `path` VARCHAR(255) NOT NULL UNIQUE,   -- full "path"
    `name` VARCHAR(32) NOT NULL UNIQUE,    -- short name/nickname
    --`type` VARCHAR(32) NOT NULL,    -- repo type (hg, git, etc)
    --`acl`...
    INDEX(`path`),
    INDEX(`name`)
) CHARACTER SET 'utf8' COLLATE 'utf8_bin';

-- SOT (Source of Truth) - all complete changesets
-- SHARDING: Doesn't look like this can be sharded if we want to batch updates and retain
-- consistency with commit atomicity.
CREATE TABLE `changeset` (
    `id` BIGINT UNSIGNED NOT NULL UNIQUE AUTO_INCREMENT,
    `repo_id` INT UNSIGNED NOT NULL,
    `cs_id` VARBINARY(32) NOT NULL,
    `gen` BIGINT UNSIGNED NOT NULL, -- index for ordering/ranges?
    PRIMARY KEY (`id`),
    UNIQUE KEY (`repo_id`, `cs_id`),
    FOREIGN KEY (`repo_id`) REFERENCES `repos` (`id`)
) CHARACTER SET 'utf8' COLLATE 'utf8_bin';

-- for n parents
-- SHARDING: same sharding issue as `changeset`
CREATE TABLE `csparents` (
    `cs_id` BIGINT UNSIGNED NOT NULL,
    `parent_id` BIGINT UNSIGNED NOT NULL,
    `seq` INT UNSIGNED NOT NULL, -- if ordered
    PRIMARY KEY (`cs_id`, `parent_id`, `seq`),
    FOREIGN KEY (`cs_id`) REFERENCES `changeset` (`id`),
    FOREIGN KEY (`parent_id`) REFERENCES `changeset` (`id`)
);

-- SOT: remap between different cs ids
-- SHARDING: probably not?
CREATE TABLE `csremap` (
    `repo_id` INT UNSIGNED NOT NULL,
    `canon` BIGINT UNSIGNED NOT NULL,
    `typetag` VARBINARY(16) NOT NULL, -- could be enum; example: 'hg-flat-sha1', 'hg-tree-sha1', 'mnk-blake2'
    `alias` VARBINARY(60) NOT NULL, -- ascii/binary?
    INDEX (`repo_id`, `canon`),
    UNIQUE KEY (`repo_id`, `alias`, `typetag`), -- index typetag last since we may want to search without knowing it
    FOREIGN KEY (`repo_id`) REFERENCES `repos` (`id`),
    FOREIGN KEY (`canon`) REFERENCES `changeset` (`id`)
);

-- bookmarks and heads updated atomically with same transaction
-- SOT bookmarks are given names for revisions
-- SHARDING: seems unlikely we'll need it
CREATE TABLE `bookmarks` (
    `repo_id` INT UNSIGNED NOT NULL,
    `name` VARCHAR(255) NOT NULL,
    `cs_id` BIGINT UNSIGNED NOT NULL,
    UNIQUE KEY (`repo_id`, `name`),
    FOREIGN KEY (`repo_id`) REFERENCES `repos` (`id`),
    FOREIGN KEY (`cs_id`) REFERENCES `changeset` (`id`)
) CHARACTER SET 'utf8' COLLATE 'utf8_bin';

-- Heads could be regenerated from complete table in principle, except if we
-- encode `visible` here (ie, "real" head vs traceroot)
-- no need to shard?
CREATE TABLE `heads` (
    `cs_id` BIGINT UNSIGNED NOT NULL,
    `visible` BIT NOT NULL, -- needed? client-only?
    UNIQUE KEY (`cs_id`),
    INDEX (`visible`, `cs_id`),
    FOREIGN KEY (`cs_id`) REFERENCES `changeset` (`id`)
);

-- SOT: obsmark, but without the implication of hiding/obsoleting
-- SHARDING: ?
CREATE TABLE `actionmark` (
    `repo_id` INT UNSIGNED NOT NULL,
    `old` BIGINT UNSIGNED NOT NULL,
    `new` BIGINT UNSIGNED NOT NULL,
    `action` VARCHAR(20), -- action taken to create the mark
    -- more fields
    PRIMARY KEY (`repo_id`, `old`, `new`), -- unique?
    INDEX (`repo_id`, `new`),
    FOREIGN KEY (`repo_id`) REFERENCES `repos` (`id`),
    FOREIGN KEY (`old`) REFERENCES `changeset` (`id`),
    FOREIGN KEY (`new`) REFERENCES `changeset` (`id`)
) CHARACTER SET 'utf8' COLLATE 'utf8_bin';

-- DERIVED DATA

-- create surrogate key for paths
-- SHARDING: not clear what key to shard on
CREATE TABLE `paths` (
    `id` BIGINT UNSIGNED NOT NULL UNIQUE AUTO_INCREMENT,
    `repo_id` INT UNSIGNED NOT NULL,
    `path` VARBINARY(4096) NOT NULL, -- paths not necessarily utf8
    `istree` BIT NOT NULL,
    PRIMARY KEY (`id`),
    UNIQUE (`repo_id`, `path`(512), `istree`),
    FOREIGN KEY (`repo_id`) REFERENCES `repos` (`id`)
) CHARACTER SET 'utf8' COLLATE 'utf8_bin';

-- Referenced by changesets (as root manifest object) and by other manifest objects.
-- Looks like we don't need a separate filenode table if we can just index here?
-- Also if this contained pathid index, then no need for filelog?
-- If this contains csid, no need for separate linknode
-- XXX Is this table too big?
-- SHARDING: could shard on unodeid
CREATE TABLE `unode` (
    `id` BIGINT UNSIGNED NOT NULL UNIQUE AUTO_INCREMENT,
    `repo_id` INT UNSIGNED NOT NULL,
    `unodehash` VARBINARY(32) NOT NULL, -- hash(path, contentid, salt)
    `contentid` VARBINARY(32) NOT NULL,
    `salt` INT UNSIGNED NOT NULL, -- larger? hash-sized if derived by hash?
    `pathid` BIGINT UNSIGNED NOT NULL, -- fastlog?
    `gen` BIGINT UNSIGNED NOT NULL, -- XXX useful? order/range index?
    PRIMARY KEY (`id`),
    UNIQUE KEY (`repo_id`, `unodehash`),
    FOREIGN KEY (`repo_id`) REFERENCES `repos` (`id`),
    FOREIGN KEY (`pathid`) REFERENCES `paths` (`id`),
    -- FOREIGN KEY (`copyfrom`) REFERENCES `fixedcopyinfo` (`id`), -- can't foreign key on NULLable?
    FOREIGN KEY (`cs_id`) REFERENCES `changeset` (`id`)
);

-- Parents for a unode.
-- SHARDING: shard on unodeid?
CREATE TABLE `unodeparents` (
    `unode_id` BIGINT UNSIGNED NOT NULL,
    `parent_id` BIGINT UNSIGNED NOT NULL,
    `seq` INT NOT NULL,
    UNIQUE KEY (`unode_id`, `parent_id`, `seq`),
    FOREIGN KEY (`unode_id`) REFERENCES `unode` (`id`),
    FOREIGN KEY (`parent_id`) REFERENCES `unode` (`id`)
);

-- 1:many relationship between filenodes and unodes
CREATE TABLE `filenode` (
  `unode` BIGINT UNSIGNED NOT NULL,
  `filenode` VARBINARY(32) NOT NULL, -- hash(p1, p2, copyfrom, content)
  PRIMARY KEY (`unode_id`, `filenode`),
  FOREIGN KEY (`unode_id`) REFERENCES `unode` (`id`)
);

-- get linkrevs for a filenode by joining filenode -> unode -> changeset
CREATE TABLE `linkrev` (
  `unode` BIGINT UNSIGNED NOT NULL,
  `cs_id` INT UNSIGNED NOT NULL,
  UNIQUE KEY (`unode`, `cs_id`),
  FOREIGN KEY (`unode`) REFERENCES `unode` (`id`),
  FOREIGN KEY (`cs_id`) REFERENCES `changeset` (`id`)
);

-- Copyinfo baked into a filenode
-- SHARDING: seems unlikely to be necessary
CREATE TABLE `fixedcopyinfo` (
    `id` INT UNSIGNED UNIQUE NOT NULL AUTO_INCREMENT,
    `copyinfo` VARBINARY(4096),
    PRIMARY KEY (`id`)
);

-- SOT - retroactively added copy information
-- SHARDING: This table could be large from mass renames. Shard on path prefix? Path hash?
CREATE TABLE `copyinfo` (
    `pathid` BIGINT UNSIGNED NOT NULL,
    `unode_id` BIGINT UNSIGNED NOT NULL,
    `kind` VARBINARY(10) NOT NULL,
    `frompath` BIGINT UNSIGNED NOT NULL,
    `fromunode` VARBINARY(32) NOT NULL,
    UNIQUE KEY (`pathid`, `unode_id`),
    FOREIGN KEY (`pathid`) REFERENCES `path` (`id`),
    FOREIGN KEY (`unode_id`) REFERENCES `unode` (`id`)
);

-- Pseudo tables representing Manifold

-- SOT - all immutable data stored as blobs
CREATE TABLE `content` (
    `repo_id` INT UNSIGNED NOT NULL,
    `contentid` VARBINARY(32) NOT NULL,
    `blob` LONGBLOB,
    UNIQUE KEY (`repo_id`, `blobid`)
) ENGINE=`manifold`;

-- SOT - aliases for blobs under various useful identities
CREATE TABLE `contentalias` (
    `repo_id` INT UNSIGNED NOT NULL,
    `contentalias` VARBINARY(40) NOT NULL,
    `aliastype` VARCHAR(16) NOT NULL,
    `contentid` VARBINARY(32) NOT NULL,
    UNIQUE KEY (`repo_id`, `contentalias`, `aliastype`),
    INDEX (`repo_id`, `contentid`)
) ENGINE='manifold';
