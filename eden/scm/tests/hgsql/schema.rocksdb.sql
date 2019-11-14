CREATE TABLE `revisions` (
  `repo` varbinary(64) NOT NULL,
  `path` varbinary(512) NOT NULL,
  `chunk` int(10) unsigned NOT NULL,
  `chunkcount` int(10) unsigned NOT NULL,
  `linkrev` int(10) unsigned NOT NULL,
  `rev` int(10) unsigned NOT NULL,
  `node` binary(40) NOT NULL,
  `entry` binary(64) NOT NULL,
  `data0` varbinary(1) NOT NULL,
  `data1` longblob NOT NULL,
  `createdtime` datetime NOT NULL,
  PRIMARY KEY (`repo`,`path`,`rev`,`chunk`),
  KEY `linkrevs` (`repo`,`linkrev`)
) ENGINE=RocksDB DEFAULT CHARSET=latin1;

CREATE TABLE `revision_references` (
  `autoid` bigint(20) unsigned NOT NULL AUTO_INCREMENT,
  `repo` varbinary(64) NOT NULL,
  `namespace` varbinary(32) NOT NULL,
  `name` varbinary(256) DEFAULT NULL,
  `value` varbinary(40) NOT NULL,
  PRIMARY KEY (`autoid`),
  UNIQUE KEY `bookmarkindex` (`repo`,`namespace`,`name`)
) ENGINE=RocksDB DEFAULT CHARSET=latin1;

CREATE TABLE `repo_lock` (
  `repo` varbinary(64) PRIMARY KEY,
  `state` tinyint NOT NULL,
  `reason` varbinary(255)
) ENGINE=RocksDB DEFAULT CHARSET=latin1;
