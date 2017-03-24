CREATE TABLE `revisions` (
  `repo` varchar(64) CHARACTER SET latin1 COLLATE latin1_bin NOT NULL,
  `path` varchar(512) CHARACTER SET latin1 COLLATE latin1_bin NOT NULL,
  `chunk` int(10) unsigned NOT NULL,
  `chunkcount` int(10) unsigned NOT NULL,
  `linkrev` int(10) unsigned NOT NULL,
  `rev` int(10) unsigned NOT NULL,
  `node` char(40) CHARACTER SET latin1 COLLATE latin1_bin NOT NULL,
  `entry` binary(64) NOT NULL,
  `data0` char(1) NOT NULL,
  `data1` longblob NOT NULL,
  `createdtime` datetime NOT NULL,
  PRIMARY KEY (`repo`,`path`,`rev`,`chunk`),
  KEY `linkrevs` (`repo`,`linkrev`)
) ENGINE=InnoDB DEFAULT CHARSET=latin1;

CREATE TABLE `revision_references` (
  `autoid` int(10) unsigned NOT NULL AUTO_INCREMENT,
  `repo` varchar(64) CHARACTER SET latin1 COLLATE latin1_bin NOT NULL,
  `namespace` varchar(32) CHARACTER SET latin1 COLLATE latin1_bin NOT NULL,
  `name` varchar(256) CHARACTER SET latin1 COLLATE latin1_bin DEFAULT NULL,
  `value` char(40) CHARACTER SET latin1 COLLATE latin1_bin NOT NULL,
  PRIMARY KEY (`autoid`),
  UNIQUE KEY `bookmarkindex` (`repo`,`namespace`,`name`)
) ENGINE=InnoDB DEFAULT CHARSET=latin1;
