CREATE TABLE `pushrebaserecording` (
  `id` bigint(20) unsigned NOT NULL AUTO_INCREMENT,
  `repo_id` int(10) unsigned NOT NULL,
  `ontorev` binary(40) NOT NULL,
  `onto` varchar(512) NOT NULL,
  `onto_rebased_rev` binary(40) DEFAULT NULL,
  `conflicts` LONGTEXT DEFAULT NULL,
  `pushrebase_errmsg` varchar(1024) DEFAULT NULL,
  `upload_errmsg` varchar(1024) DEFAULT NULL,
  `bundlehandle` varchar(1024),
  `timestamps` LONGTEXT NOT NULL,
  `recorded_manifest_hashes` LONGTEXT NOT NULL,
  `real_manifest_hashes` LONGTEXT NOT NULL,
  `duration_ms` int(10),
  `replacements_revs` varchar(1024) DEFAULT NULL,
  `ordered_added_revs` varchar(1024) DEFAULT NULL,
PRIMARY KEY (`id`) )
ENGINE=InnoDB DEFAULT CHARSET=utf8;
