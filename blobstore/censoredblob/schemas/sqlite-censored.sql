CREATE TABLE  `censored_contents` (
	`id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
	`content_key` VARCHAR(255) NOT NULL,
	`task` VARCHAR(64) NOT NULL,
	`add_timestamp` BIGINT(20) NOT NULL
);

CREATE INDEX `content_key`
ON `censored_contents` (`content_key`);
