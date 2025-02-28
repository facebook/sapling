# EdenStats Counter/Duration

These stats are all listed in `EdenStats.h` file. There are two type of stats in
this file:

- `Counter` : The static counters can call `increment()` method to add a number

- `Duration` : These stats record duration of the events with `addDuration()`
  method. The duration names should ends with `_us` to highlight that the value
  is in microseconds.
  > ## Note: These stats get turned into a histogram, and EdenFS reports the following for them
  >
  > - Export Types
  >   - count(the number of times that `increment()` or `addDuration()` get
  >     called)
  >   - sum(accumulated value in the counter/duration)
  >   - average (sum/count)
  >   - rate
  > - Sliding window: all these four export types are reported on sliding
  >   windows of
  >   - 1 min, 10 min, and 1 hour
  > - Only `Durations` are turned into the following percentiles
  >   - P1, P10, P50, P90, and P99

The list of all the EdenStats Counter/Duration are as follows:

- [SaplingBackingStoreStats](./SaplingBackingStoreStats.md)
- [ObjectStoreStats](./ObjectStoreStats.md)
- [LocalStoreStats](./LocalStoreStats.md)
- [OverlayStats](./OverlayStats.md)
- JournalStats

  1. `Counter truncatedReads{"journal.truncated_reads"}` : Number of times a
     truncated read happens in Journal.

  2. `Counter filesAccumulated{"journal.files_accumulated"}` : Number of files
     accumulated in Journal.

  3. `Duration accumulateRange{"journal.accumulate_range_us"}` : The duration of
     the journal accumulates range function.

  4. `Counter journalStatusCacheHit{"journal.status_cache_hit"}` : Number of
     cache hits. This is updated when we have a valid SCM status result in cache
     to return given the current Journal sequence number.

  5. `Counter journalStatusCacheMiss{"journal.status_cache_miss"}` : Number of
     cache misses. This is updated when we don't have a valid SCM status result
     in cache to return given the current Journal sequence number.

  6. `Counter journalStatusCacheSkip{"journal.status_cache_skip"}` : Number of
     cache insertion skipped. This is updated when we skip inserting a new entry
     into the cache when the number of the entries from the calculated result is
     larger than the limit configured [here](https://fburl.com/code/flwry2g4).

- ThriftStats

  1. `Duration streamChangesSince{ "thrift.StreamingEdenService.streamChangesSince.streaming_time_us"}`
     : Duration of thrift stream change calls.

  2. `Duration streamSelectedChangesSince{"thrift.StreamingEdenService.streamSelectedChangesSince.streaming_time_us"}`
     : Duration of thrift stream change calls for selected changes.

  3. `Counter globFilesSaplingRemoteAPISuccess{"thrift.EdenServiceHandler.glob_files.sapling_remote_api_success"}`
     : Count number of times globFiles succeed using remote pathway

  4. `Counter globFilesSaplingRemoteAPIFallback{"thrift.EdenServiceHandler.glob_files.sapling_remote_api_fallback"}`
     : Count number of times globFiles fails using the remote pathway and ends
     up using the fallback pathway

  5. `Counter globFilesLocal{"thrift.EdenServiceHandler.glob_files.local_success"}`
     : Count number of times globFiles succeed using the indicated local pathway

  6. `Duration globFilesSaplingRemoteAPISuccessDuration{"thrift.EdenServiceHandler.glob_files.sapling_remote_api_success_duration_us"}`
     : Duration for how long it takes globFiles to execute the remote pathway

  7. `Duration globFilesSaplingRemoteAPIFallbackDuration{"thrift.EdenServiceHandler.glob_files.sapling_remote_api_fallback_duration_us"}`
     : Duration for how long it takes globFiles to execute the fallback pathway

  8. `Duration globFilesLocalDuration{"thrift.EdenServiceHandler.glob_files.local_duration_us"}`
     : Duration for how long it takes globFiles to execute in the indicated
     local pathway

  9. `Duration globFilesLocalOffloadableDuration{"thrift.EdenServiceHandler.glob_files.local_offloadable_duration_us"}`
     : Duration for how long it takes globFiles to execute a potentially
     offloadable request locally

- InodeMapStats

  1. `Counter lookupTreeInodeHit{"inode_map.lookup_tree_inode_hit"}` : Count the
     number of Tree Inodes found in the InodeMap

  2. `Counter lookupBlobInodeHit{"inode_map.lookup_blob_inode_hit"}` : Count the
     number of Blob Inodes found in the InodeMap

  3. `Counter lookupTreeInodeMiss{"inode_map.lookup_tree_inode_miss"}` : Count
     the number of Tree Inodes missed in the InodeMap

  4. `Counter lookupBlobInodeMiss{"inode_map.lookup_blob_inode_miss"}` : Count
     the number of Blob Inodes missed in the InodeMap

  5. `Counter lookupInodeError{"inode_map.lookup_inode_error"}` : Count the
     number of Inodes lookup errors

- InodeMetadataTableStats

  1. `Counter getHit{"inode_metadata_table.get_hit"}` : Count the number of hits
     in InodeMetadata Table

  2. `Counter getMiss{"inode_metadata_table.get_miss"}` : Count the number of
     misses in InodeMetadata Table

- BlobCacheStats

  1. `Counter getHit{"blob_cache.get_hit"}` : Number of times BlobCache request
     got hit

  2. `Counter getMiss{"blob_cache.get_miss"}` : Number of times BlobCache
     request got miss

  3. `Counter insertEviction{"blob_cache.insert_eviction"}` : Number of blobs
     evicted from cache (The cache reaches its maximum size and the LRU (least
     recently used) item evicted from cache)

  4. `Counter objectDrop{"blob_cache.object_drop"}` : Number of blobs dropped
     from cache (For some reason the object was invalid, and it got dropped from
     the cache)

- TreeCacheStats

  1. `Counter getHit{"tree_cache.get_hit"}` : Number of times TreeCache request
     got hit

  2. `Counter getMiss{"tree_cache.get_miss"}` : Number of times TreeCache
     request got miss

  3. `Counter insertEviction{"tree_cache.insert_eviction"}` : Number of trees
     evicted from cache (The cache reaches its maximum size and the LRU (least
     recently used) item evicted from cache)

  4. `Counter objectDrop{"tree_cache.object_drop"}` : Number of trees dropped
     from cache (For some reason the object was invalid and it got dropped from
     the cache)

- FakeStats
  - This is a fake stats object that is used for testing. Counter/Duration
    objects can be added here to mirror variables used in real stats objects as
    needed.
- FuseStats

  - In Fuse FS the following ODS Durations record the duration of each Fuse
    command in microseconds. Also, we have counters for all these durations for
    Successful/Failure events.

  ```
  Duration lookup{"fuse.lookup_us"}
  Duration forget{"fuse.forget_us"}
  Duration getattr{"fuse.getattr_us"}
  Duration setattr{"fuse.setattr_us"}
  Duration readlink{"fuse.readlink_us"}
  Duration mknod{"fuse.mknod_us"}
  Duration mkdir{"fuse.mkdir_us"}
  Duration unlink{"fuse.unlink_us"}
  Duration rmdir{"fuse.rmdir_us"}
  Duration symlink{"fuse.symlink_us"}
  Duration rename{"fuse.rename_us"}
  Duration link{"fuse.link_us"}
  Duration open{"fuse.open_us"}
  Duration read{"fuse.read_us"}
  Duration write{"fuse.write_us"}
  Duration flush{"fuse.flush_us"}
  Duration release{"fuse.release_us"}
  Duration fsync{"fuse.fsync_us"}
  Duration opendir{"fuse.opendir_us"}
  Duration readdir{"fuse.readdir_us"}
  Duration releasedir{"fuse.releasedir_us"}
  Duration fsyncdir{"fuse.fsyncdir_us"}
  Duration statfs{"fuse.statfs_us"}
  Duration setxattr{"fuse.setxattr_us"}
  Duration getxattr{"fuse.getxattr_us"}
  Duration listxattr{"fuse.listxattr_us"}
  Duration removexattr{"fuse.removexattr_us"}
  Duration access{"fuse.access_us"}
  Duration create{"fuse.create_us"}
  Duration bmap{"fuse.bmap_us"}
  Duration forgetmulti{"fuse.forgetmulti_us"}
  Duration fallocate{"fuse.fallocate_us"}
  ```

- NfsStats

  - In NFS the following ODS Durations record the duration of each NFS command
    in microseconds. Also, we have counters for all of these duration for
    Successful/Failure events.

  ```
  Duration nfsNull{"nfs.null_us"}
  Duration nfsGetattr{"nfs.getattr_us"}
  Duration nfsSetattr{"nfs.setattr_us"}
  Duration nfsLookup{"nfs.lookup_us"}
  Duration nfsAccess{"nfs.access_us"}
  Duration nfsReadlink{"nfs.readlink_us"}
  Duration nfsRead{"nfs.read_us"}
  Duration nfsWrite{"nfs.write_us"}
  Duration nfsCreate{"nfs.create_us"}
  Duration nfsMkdir{"nfs.mkdir_us"}
  Duration nfsSymlink{"nfs.symlink_us"}
  Duration nfsMknod{"nfs.mknod_us"}
  Duration nfsRemove{"nfs.remove_us"}
  Duration nfsRmdir{"nfs.rmdir_us"}
  Duration nfsRename{"nfs.rename_us"}
  Duration nfsLink{"nfs.link_us"}
  Duration nfsReaddir{"nfs.readdir_us"}
  Duration nfsReaddirplus{"nfs.readdirplus_us"}
  Duration nfsFsstat{"nfs.fsstat_us"}
  Duration nfsFsinfo{"nfs.fsinfo_us"}
  Duration nfsPathconf{"nfs.pathconf_us"}
  Duration nfsCommit{"nfs.commit_us"}
  ```

- PrjfsStats
  - In prjFS the following ODS Durations record the duration of each command in
    microseconds. Also, we have counters for all of these duration for
    Successful/Failure events.
  ```
  Duration newFileCreated{"prjfs.newFileCreated_us"}
  Duration fileOverwritten{"prjfs.fileOverwritten_us"}
  Duration fileHandleClosedFileModified{"prjfs.fileHandleClosedFileModified_us"}
  Duration fileRenamed{"prjfs.fileRenamed_us"}
  Duration preDelete{"prjfs.preDelete_us"}
  Duration preRenamed{"prjfs.preRenamed_us"}
  Duration fileHandleClosedFileDeleted{"prjfs.fileHandleClosedFileDeleted_us"}
  Duration preSetHardlink{"prjfs.preSetHardlink_us"}
  Duration preConvertToFull{"prjfs.preConvertToFull_us"}
  Duration openDir{"prjfs.opendir_us"}
  Duration readDir{"prjfs.readdir_us"}
  Duration lookup{"prjfs.lookup_us"}
  Duration access{"prjfs.access_us"}
  Duration read{"prjfs.read_us"}
  Duration removeCachedFile{"prjfs.remove_cached_file_us"}
  Duration addDirectoryPlaceholder{"prjfs.add_directory_placeholder_us"}
  ```
