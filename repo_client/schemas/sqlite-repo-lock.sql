CREATE TABLE `repo_lock` (
  `repo` VARCHAR(255) PRIMARY KEY,
  `state` INTEGER NOT NULL,
  `reason` VARCHAR(255)
);
