/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <memory>

#include <folly/ThreadLocal.h>

#include "eden/common/telemetry/DurationScope.h"
#include "eden/common/telemetry/Stats.h"
#include "eden/common/telemetry/StatsGroup.h"
#include "eden/common/utils/RefPtr.h"
#include "eden/fs/eden-config.h"

namespace facebook::eden {

struct FuseStats;
struct NfsStats;
struct PrjfsStats;
struct ObjectStoreStats;
struct LocalStoreStats;
struct SaplingBackingStoreStats;
struct JournalStats;
struct ThriftStats;
struct OverlayStats;
struct InodeMapStats;
struct InodeMetadataTableStats;
struct BlobCacheStats;
struct TreeCacheStats;
struct FakeStats;

class EdenStats : public RefCounted {
 public:
  /**
   * Records a specified elapsed duration. Updates thread-local storage, and
   * aggregates into the fb303 ServiceData in the background and on reads.
   */
  template <typename T, typename Rep, typename Period>
  void addDuration(
      StatsGroupBase::Duration T::*duration,
      std::chrono::duration<Rep, Period> elapsed) {
    (getStatsForCurrentThread<T>().*duration).addDuration(elapsed);
  }

  template <typename T>
  void increment(StatsGroupBase::Counter T::*counter, double value = 1.0) {
    (getStatsForCurrentThread<T>().*counter).addValue(value);
  }

  /**
   * Aggregates thread-locals into fb303's ServiceData.
   *
   * This function can be called on any thread.
   */
  void flush();

  template <typename T>
  T& getStatsForCurrentThread() = delete;

 private:
  class ThreadLocalTag {};

  template <typename T>
  using ThreadLocal = folly::ThreadLocal<T, ThreadLocalTag, void>;

  ThreadLocal<FuseStats> fuseStats_;
  ThreadLocal<NfsStats> nfsStats_;
  ThreadLocal<PrjfsStats> prjfsStats_;
  ThreadLocal<ObjectStoreStats> objectStoreStats_;
  ThreadLocal<LocalStoreStats> localStoreStats_;
  ThreadLocal<SaplingBackingStoreStats> saplingBackingStoreStats_;
  ThreadLocal<JournalStats> journalStats_;
  ThreadLocal<ThriftStats> thriftStats_;
  ThreadLocal<TelemetryStats> telemetryStats_;
  ThreadLocal<OverlayStats> overlayStats_;
  ThreadLocal<InodeMapStats> inodeMapStats_;
  ThreadLocal<InodeMetadataTableStats> inodeMetadataTableStats_;
  ThreadLocal<BlobCacheStats> blobCacheStats_;
  ThreadLocal<TreeCacheStats> treeCacheStats_;
  ThreadLocal<FakeStats> fakeStats_;
};

using EdenStatsPtr = RefPtr<EdenStats>;

template <>
inline FuseStats& EdenStats::getStatsForCurrentThread<FuseStats>() {
  return *fuseStats_.get();
}

template <>
inline NfsStats& EdenStats::getStatsForCurrentThread<NfsStats>() {
  return *nfsStats_.get();
}

template <>
inline PrjfsStats& EdenStats::getStatsForCurrentThread<PrjfsStats>() {
  return *prjfsStats_.get();
}

template <>
inline ObjectStoreStats&
EdenStats::getStatsForCurrentThread<ObjectStoreStats>() {
  return *objectStoreStats_.get();
}

template <>
inline LocalStoreStats& EdenStats::getStatsForCurrentThread<LocalStoreStats>() {
  return *localStoreStats_.get();
}

template <>
inline SaplingBackingStoreStats&
EdenStats::getStatsForCurrentThread<SaplingBackingStoreStats>() {
  return *saplingBackingStoreStats_.get();
}

template <>
inline JournalStats& EdenStats::getStatsForCurrentThread<JournalStats>() {
  return *journalStats_.get();
}

template <>
inline ThriftStats& EdenStats::getStatsForCurrentThread<ThriftStats>() {
  return *thriftStats_.get();
}

template <>
inline TelemetryStats& EdenStats::getStatsForCurrentThread<TelemetryStats>() {
  return *telemetryStats_.get();
}

template <>
inline OverlayStats& EdenStats::getStatsForCurrentThread<OverlayStats>() {
  return *overlayStats_.get();
}

template <>
inline InodeMapStats& EdenStats::getStatsForCurrentThread<InodeMapStats>() {
  return *inodeMapStats_.get();
}

template <>
inline InodeMetadataTableStats&
EdenStats::getStatsForCurrentThread<InodeMetadataTableStats>() {
  return *inodeMetadataTableStats_.get();
}

template <>
inline BlobCacheStats& EdenStats::getStatsForCurrentThread<BlobCacheStats>() {
  return *blobCacheStats_.get();
}

template <>
inline TreeCacheStats& EdenStats::getStatsForCurrentThread<TreeCacheStats>() {
  return *treeCacheStats_.get();
}

template <>
inline FakeStats& EdenStats::getStatsForCurrentThread<FakeStats>() {
  return *fakeStats_.get();
}

struct FuseStats : StatsGroup<FuseStats> {
  Duration lookup{"fuse.lookup_us"};
  Duration forget{"fuse.forget_us"};
  Duration getattr{"fuse.getattr_us"};
  Duration setattr{"fuse.setattr_us"};
  Duration readlink{"fuse.readlink_us"};
  Duration mknod{"fuse.mknod_us"};
  Duration mkdir{"fuse.mkdir_us"};
  Duration unlink{"fuse.unlink_us"};
  Duration rmdir{"fuse.rmdir_us"};
  Duration symlink{"fuse.symlink_us"};
  Duration rename{"fuse.rename_us"};
  Duration link{"fuse.link_us"};
  Duration open{"fuse.open_us"};
  Duration read{"fuse.read_us"};
  Duration write{"fuse.write_us"};
  Duration flush{"fuse.flush_us"};
  Duration release{"fuse.release_us"};
  Duration fsync{"fuse.fsync_us"};
  Duration opendir{"fuse.opendir_us"};
  Duration readdir{"fuse.readdir_us"};
  Duration releasedir{"fuse.releasedir_us"};
  Duration fsyncdir{"fuse.fsyncdir_us"};
  Duration statfs{"fuse.statfs_us"};
  Duration setxattr{"fuse.setxattr_us"};
  Duration getxattr{"fuse.getxattr_us"};
  Duration listxattr{"fuse.listxattr_us"};
  Duration removexattr{"fuse.removexattr_us"};
  Duration access{"fuse.access_us"};
  Duration create{"fuse.create_us"};
  Duration bmap{"fuse.bmap_us"};
  Duration ioctl{"fuse.ioctl_us"};
  Duration poll{"fuse.poll_us"};
  Duration forgetmulti{"fuse.forgetmulti_us"};
  Duration fallocate{"fuse.fallocate_us"};
};

struct NfsStats : StatsGroup<NfsStats> {
  Duration nfsNull{"nfs.null_us"};
  Duration nfsGetattr{"nfs.getattr_us"};
  Duration nfsSetattr{"nfs.setattr_us"};
  Duration nfsLookup{"nfs.lookup_us"};
  Duration nfsAccess{"nfs.access_us"};
  Duration nfsReadlink{"nfs.readlink_us"};
  Duration nfsRead{"nfs.read_us"};
  Duration nfsWrite{"nfs.write_us"};
  Duration nfsCreate{"nfs.create_us"};
  Duration nfsMkdir{"nfs.mkdir_us"};
  Duration nfsSymlink{"nfs.symlink_us"};
  Duration nfsMknod{"nfs.mknod_us"};
  Duration nfsRemove{"nfs.remove_us"};
  Duration nfsRmdir{"nfs.rmdir_us"};
  Duration nfsRename{"nfs.rename_us"};
  Duration nfsLink{"nfs.link_us"};
  Duration nfsReaddir{"nfs.readdir_us"};
  Duration nfsReaddirplus{"nfs.readdirplus_us"};
  Duration nfsFsstat{"nfs.fsstat_us"};
  Duration nfsFsinfo{"nfs.fsinfo_us"};
  Duration nfsPathconf{"nfs.pathconf_us"};
  Duration nfsCommit{"nfs.commit_us"};
};

struct PrjfsStats : StatsGroup<PrjfsStats> {
  Counter outOfOrderCreate{"prjfs.out_of_order_create"};
  Duration queuedFileNotification{"prjfs.queued_file_notification_us"};
  Duration filesystemSync{"prjfs.filesystem_sync_us"};

  Duration newFileCreated{"prjfs.newFileCreated_us"};
  Duration fileOverwritten{"prjfs.fileOverwritten_us"};
  Duration fileHandleClosedFileModified{
      "prjfs.fileHandleClosedFileModified_us"};
  Duration fileRenamed{"prjfs.fileRenamed_us"};
  Duration preDelete{"prjfs.preDelete_us"};
  Duration preRenamed{"prjfs.preRenamed_us"};
  Duration fileHandleClosedFileDeleted{"prjfs.fileHandleClosedFileDeleted_us"};
  Duration preSetHardlink{"prjfs.preSetHardlink_us"};
  Duration preConvertToFull{"prjfs.preConvertToFull_us"};

  Duration openDir{"prjfs.opendir_us"};
  Duration readDir{"prjfs.readdir_us"};
  Duration lookup{"prjfs.lookup_us"};
  Duration access{"prjfs.access_us"};
  Duration read{"prjfs.read_us"};

  Duration removeCachedFile{"prjfs.remove_cached_file_us"};
  Duration addDirectoryPlaceholder{"prjfs.add_directory_placeholder_us"};
};

/**
 * @see ObjectStore
 */
struct ObjectStoreStats : StatsGroup<ObjectStoreStats> {
  Duration getTree{"store.get_tree_us"};
  Duration getBlob{"store.get_blob_us"};
  Duration getBlobMetadata{"store.get_blob_metadata_us"};
  Duration getRootTree{"store.get_root_tree_us"};

  Counter getBlobFromMemory{"object_store.get_blob.memory"};
  Counter getBlobFromLocalStore{"object_store.get_blob.local_store"};
  Counter getBlobFromBackingStore{"object_store.get_blob.backing_store"};
  Counter getBlobFailed{"object_store.get_blob_failed"};

  Counter getTreeFromMemory{"object_store.get_tree.memory"};
  Counter getTreeFromLocalStore{"object_store.get_tree.local_store"};
  Counter getTreeFromBackingStore{"object_store.get_tree.backing_store"};
  Counter getTreeFailed{"object_store.get_tree_failed"};

  Counter getRootTreeFromBackingStore{
      "object_store.get_root_tree.backing_store"};
  Counter getRootTreeFailed{"object_store.get_root_tree_failed"};

  Counter getBlobMetadataFromMemory{"object_store.get_blob_metadata.memory"};
  Counter getBlobMetadataFromLocalStore{
      "object_store.get_blob_metadata.local_store"};
  Counter getBlobMetadataFromBackingStore{
      "object_store.get_blob_metadata.backing_store"};
  Counter getBlobMetadataFromBlob{"object_store.get_blob_metadata.blob"};
  Counter getBlobMetadataFailed{"object_store.get_blob_metadata_failed"};
};

struct LocalStoreStats : StatsGroup<LocalStoreStats> {
  Duration getTree{"local_store.get_tree_us"};
  Duration getBlob{"local_store.get_blob_us"};
  Duration getBlobMetadata{"local_store.get_blob_metadata_us"};
  Counter getTreeSuccess{"local_store.get_tree_success"};
  Counter getBlobSuccess{"local_store.get_blob_success"};
  Counter getBlobMetadataSuccess{"local_store.get_blob_metadata_success"};
  Counter getTreeFailure{"local_store.get_tree_failure"};
  Counter getBlobFailure{"local_store.get_blob_failure"};
  Counter getBlobMetadataFailure{"local_store.get_blob_metadata_failure"};
  Counter getTreeError{"local_store.get_tree_error"};
  Counter getBlobError{"local_store.get_blob_error"};
  Counter getBlobMetadataError{"local_store.get_blob_metadata_error"};
};

/**
 * @see SaplingBackingStore
 *
 * Terminology:
 *   get = entire lookup process, including both Sapling disk hits and fetches
 *   fetch = includes asynchronous retrieval from Mononoke
 */
struct SaplingBackingStoreStats : StatsGroup<SaplingBackingStoreStats> {
  Duration getTree{"store.sapling.get_tree_us"};
  Duration fetchTree{"store.sapling.fetch_tree_us"};
  Duration getRootTree{"store.sapling.get_root_tree_us"};
  Duration importManifestForRoot{"store.sapling.import_manifest_for_root_us"};
  Counter fetchTreeLocal{"store.sapling.fetch_tree_local"};
  Counter fetchTreeRemote{"store.sapling.fetch_tree_remote"};
  Counter fetchTreeSuccess{"store.sapling.fetch_tree_success"};
  Counter fetchTreeFailure{"store.sapling.fetch_tree_failure"};
  Counter fetchTreeRetrySuccess{"store.sapling.fetch_tree_retry_success"};
  Counter fetchTreeRetryFailure{"store.sapling.fetch_tree_retry_failure"};
  Counter getRootTreeLocal{"store.sapling.get_root_tree_local"};
  Counter getRootTreeRemote{"store.sapling.get_root_tree_remote"};
  Counter getRootTreeSuccess{"store.sapling.get_root_tree_success"};
  Counter getRootTreeFailure{"store.sapling.get_root_tree_failure"};
  Counter getRootTreeRetrySuccess{"store.sapling.get_root_tree_retry_success"};
  Counter getRootTreeRetryFailure{"store.sapling.get_root_tree_retry_failure"};
  Counter importManifestForRootLocal{
      "store.sapling.import_manifest_for_root_local"};
  Counter importManifestForRootRemote{
      "store.sapling.import_manifest_for_root_remote"};
  Counter importManifestForRootSuccess{
      "store.sapling.import_manifest_for_root_success"};
  Counter importManifestForRootFailure{
      "store.sapling.import_manifest_for_root_failure"};
  Counter importManifestForRootRetrySuccess{
      "store.sapling.import_manifest_for_root_retry_success"};
  Counter importManifestForRootRetryFailure{
      "store.sapling.import_manifest_for_root_retry_failure"};
  Duration getBlob{"store.sapling.get_blob_us"};
  Duration fetchBlob{"store.sapling.fetch_blob_us"};
  Counter fetchBlobLocal{"store.sapling.fetch_blob_local"};
  Counter fetchBlobRemote{"store.sapling.fetch_blob_remote"};
  Counter fetchBlobSuccess{"store.sapling.fetch_blob_success"};
  Counter fetchBlobFailure{"store.sapling.fetch_blob_failure"};
  Counter fetchBlobRetrySuccess{"store.sapling.fetch_blob_retry_success"};
  Counter fetchBlobRetryFailure{"store.sapling.fetch_blob_retry_failure"};
  Duration prefetchBlob{"store.sapling.prefetch_blob_us"};
  Counter prefetchBlobLocal{"store.sapling.prefetch_blob_local"};
  Counter prefetchBlobRemote{"store.sapling.prefetch_blob_remote"};
  Counter prefetchBlobSuccess{"store.sapling.prefetch_blob_success"};
  Counter prefetchBlobFailure{"store.sapling.prefetch_blob_failure"};
  Counter prefetchBlobRetrySuccess{"store.sapling.prefetch_blob_retry_success"};
  Counter prefetchBlobRetryFailure{"store.sapling.prefetch_blob_retry_failure"};
  Duration getBlobMetadata{"store.sapling.get_blob_metadata_us"};
  Duration fetchBlobMetadata{"store.sapling.fetch_blob_metadata_us"};
  Counter fetchBlobMetadataLocal{"store.sapling.fetch_blob_metadata_local"};
  Counter fetchBlobMetadataRemote{"store.sapling.fetch_blob_metadata_remote"};
  Counter fetchBlobMetadataSuccess{"store.sapling.fetch_blob_metadata_success"};
  Counter fetchBlobMetadataFailure{"store.sapling.fetch_blob_metadata_failure"};
  Counter loadProxyHash{"store.sapling.load_proxy_hash"};
};

struct JournalStats : StatsGroup<JournalStats> {
  Counter truncatedReads{"journal.truncated_reads"};
  Counter filesAccumulated{"journal.files_accumulated"};
  Duration accumulateRange{"journal.accumulate_range_us"};
};

struct ThriftStats : StatsGroup<ThriftStats> {
  Duration streamChangesSince{
      "thrift.StreamingEdenService.streamChangesSince.streaming_time_us"};

  Duration streamSelectedChangesSince{
      "thrift.StreamingEdenService.streamSelectedChangesSince.streaming_time_us"};
};

struct OverlayStats : StatsGroup<OverlayStats> {
  Duration saveOverlayDir{"overlay.save_overlay_dir_us"};
  Duration loadOverlayDir{"overlay.load_overlay_dir_us"};
  Duration openOverlayFile{"overlay.open_overlay_file_us"};
  Duration createOverlayFile{"overlay.create_overlay_file_us"};
  Duration removeOverlayFile{"overlay.remove_overlay_file_us"};
  Duration removeOverlayDir{"overlay.remove_overlay_dir_us"};
  Duration recursivelyRemoveOverlayDir{
      "overlay.recursively_remove_overlay_dir_us"};
  Duration hasOverlayDir{"overlay.has_overlay_dir_us"};
  Duration hasOverlayFile{"overlay.has_overlay_file_us"};
  Duration addChild{"overlay.add_child_us"};
  Duration removeChild{"overlay.remove_child_us"};
  Duration removeChildren{"overlay.remove_children_us"};
  Duration renameChild{"overlay.rename_child_us"};
  Counter loadOverlayDirSuccessful{"overlay.load_overlay_dir_successful"};
  Counter loadOverlayDirFailure{"overlay.load_overlay_dir_failure"};
  Counter saveOverlayDirSuccessful{"overlay.save_overlay_dir_successful"};
  Counter saveOverlayDirFailure{"overlay.save_overlay_dir_failure"};
  Counter openOverlayFileSuccessful{"overlay.open_overlay_file_successful"};
  Counter openOverlayFileFailure{"overlay.open_overlay_file_failure"};
  Counter createOverlayFileSuccessful{"overlay.create_overlay_file_successful"};
  Counter createOverlayFileFailure{"overlay.create_overlay_file_failure"};
  Counter removeOverlayFileSuccessful{"overlay.remove_overlay_file_successful"};
  Counter removeOverlayFileFailure{"overlay.remove_overlay_file_failure"};
  Counter removeOverlayDirSuccessful{"overlay.remove_overlay_dir_successful"};
  Counter removeOverlayDirFailure{"overlay.remove_overlay_dir_failure"};
  Counter recursivelyRemoveOverlayDirSuccessful{
      "overlay.recursively_remove_overlay_dir_successful"};
  Counter recursivelyRemoveOverlayDirFailure{
      "overlay.recursively_remove_overlay_dir_failure"};
  Counter hasOverlayDirSuccessful{"overlay.has_overlay_dir_successful"};
  Counter hasOverlayDirFailure{"overlay.has_overlay_dir_failure"};
  Counter hasOverlayFileSuccessful{"overlay.has_overlay_file_successful"};
  Counter hasOverlayFileFailure{"overlay.has_overlay_file_failure"};
  Counter addChildSuccessful{"overlay.add_child_successful"};
  Counter addChildFailure{"overlay.add_child_failure"};
  Counter removeChildSuccessful{"overlay.remove_child_successful"};
  Counter removeChildFailure{"overlay.remove_child_failure"};
  Counter removeChildrenSuccessful{"overlay.remove_children_successful"};
  Counter removeChildrenFailure{"overlay.remove_children_failure"};
  Counter renameChildSuccessful{"overlay.rename_child_successful"};
  Counter renameChildFailure{"overlay.rename_child_failure"};
};

struct InodeMapStats : StatsGroup<InodeMapStats> {
  Counter lookupTreeInodeHit{"inode_map.lookup_tree_inode_hit"};
  Counter lookupBlobInodeHit{"inode_map.lookup_blob_inode_hit"};
  Counter lookupTreeInodeMiss{"inode_map.lookup_tree_inode_miss"};
  Counter lookupBlobInodeMiss{"inode_map.lookup_blob_inode_miss"};
  Counter lookupInodeError{"inode_map.lookup_inode_error"};
};

struct InodeMetadataTableStats : StatsGroup<InodeMetadataTableStats> {
  Counter getHit{"inode_metadata_table.get_hit"};
  Counter getMiss{"inode_metadata_table.get_miss"};
};

struct BlobCacheStats : StatsGroup<BlobCacheStats> {
  Counter getHit{"blob_cache.get_hit"};
  Counter getMiss{"blob_cache.get_miss"};
  Counter insertEviction{"blob_cache.insert_eviction"};
  Counter objectDrop{"blob_cache.object_drop"};
};

struct TreeCacheStats : StatsGroup<TreeCacheStats> {
  Counter getHit{"tree_cache.get_hit"};
  Counter getMiss{"tree_cache.get_miss"};
  Counter insertEviction{"tree_cache.insert_eviction"};
  Counter objectDrop{"tree_cache.object_drop"};
};

/*
 * This is a fake stats object that is used for testing. Counter/Duration
 * objects can be added here to mirror variables used in real stats object as
 * needed
 */
struct FakeStats : StatsGroup<FakeStats> {
  Counter getHit{"do_not_export_0"};
  Counter getMiss{"do_not_export_1"};
  Counter insertEviction{"do_not_export_2"};
  Counter objectDrop{"do_not_export_3"};
};

} // namespace facebook::eden
