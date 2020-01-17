CREATE TABLE `blobstore_sync_queue` (
  `id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
  `blobstore_key` varchar NOT NULL,
  `blobstore_id` INTEGER NOT NULL,
  `add_timestamp` BIGINT NOT NULL
);
