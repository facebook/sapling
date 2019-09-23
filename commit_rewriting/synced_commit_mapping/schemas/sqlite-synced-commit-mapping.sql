CREATE TABLE `synced_commit_mapping` (
  `mapping_id` INTEGER PRIMARY KEY,
  `small_repo_id` int(11) NOT NULL,
  `small_bcs_id` binary(32) NOT NULL,
  `large_repo_id` int(11) NOT NULL,
  `large_bcs_id` binary(32) NOT NULL,
  UNIQUE (`large_repo_id`,`small_repo_id`,`small_bcs_id`),
  UNIQUE (`small_repo_id`,`large_repo_id`,`large_bcs_id`)
);
