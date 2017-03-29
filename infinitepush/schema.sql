CREATE TABLE `bookmarkstonode` (
  `node` char(40) CHARACTER SET ascii COLLATE ascii_bin NOT NULL,
  `bookmark` varchar(512) CHARACTER SET ascii COLLATE ascii_bin NOT NULL,
  `reponame` varchar(255) CHARACTER SET ascii COLLATE ascii_bin NOT NULL,
  PRIMARY KEY (`reponame`,`bookmark`)
) ENGINE=InnoDB DEFAULT CHARSET=ascii;

CREATE TABLE `bundles` (
  `bundle` varchar(512) CHARACTER SET ascii COLLATE ascii_bin NOT NULL,
  `reponame` varchar(255) CHARACTER SET ascii COLLATE ascii_bin NOT NULL,
  PRIMARY KEY (`bundle`,`reponame`)
) ENGINE=InnoDB DEFAULT CHARSET=ascii;

CREATE TABLE `nodestobundle` (
  `node` char(40) CHARACTER SET ascii COLLATE ascii_bin NOT NULL,
  `bundle` varchar(512) CHARACTER SET ascii COLLATE ascii_bin NOT NULL,
  `reponame` varchar(255) CHARACTER SET ascii COLLATE ascii_bin NOT NULL,
  PRIMARY KEY (`node`,`reponame`)
) ENGINE=InnoDB DEFAULT CHARSET=ascii;
