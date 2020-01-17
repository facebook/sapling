CREATE TABLE `data` (
  `id` VARCHAR(255) NOT NULL,
  `type` TINYINT NOT NULL,
  `value` BLOB NOT NULL,
  PRIMARY KEY (`id`)
);

CREATE TABLE `chunk` (
  `id` VARCHAR(255) NOT NULL,
  `chunk_id` INT UNSIGNED NOT NULL,
  `value` BLOB NOT NULL,
  PRIMARY KEY (`id`, `chunk_id`)
);
