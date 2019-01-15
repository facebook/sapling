CREATE TABLE `data` (
  `repo_id` INT UNSIGNED NOT NULL,
  `id` VARCHAR(255) NOT NULL,
  `type` TINYINT NOT NULL,
  `value` BLOB NOT NULL,
  PRIMARY KEY (`repo_id`, `id`)
);

CREATE TABLE `chunk` (
  `repo_id` INT UNSIGNED NOT NULL,
  `id` VARCHAR(255) NOT NULL,
  `chunk_id` INT UNSIGNED NOT NULL,
  `value` BLOB NOT NULL,
  PRIMARY KEY (`repo_id`, `id`, `chunk_id`)
);
