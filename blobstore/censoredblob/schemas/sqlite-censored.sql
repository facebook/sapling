CREATE TABLE  `censored_contents` (
	`id` BIGINT(20) unsigned NOT NULL AUTO_INCREMENT,
	`content_key` VARCHAR(255) NOT NULL,
	`task` VARCHAR(64) NOT NULL,
	`add_timestamp` BIGINT(20) NOT NULL,
  PRIMARY KEY (`id`),
	INDEX `content_key` (`content_key`)
)
