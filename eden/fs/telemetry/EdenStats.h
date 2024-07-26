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

/**
 * All the EdenFS stats are documented in the EdenFS wiki
 * https://www.internalfb.com/intern/wiki/EdenFS/Development_Tips/EdenFS_ODS_Counters_and_Duration/
 * as well as the .md files in eden/fs/docs/stats/EdenStats.md
 * if you are adding or editing stats, please consider updating
 * the wiki and the .md files.
 */

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
struct ScmStatusCacheStats;
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

  template <typename T>
  std::string_view getName(StatsGroupBase::Counter T::*counter) {
    return (getStatsForCurrentThread<T>().*counter).getName();
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
  ThreadLocal<ScmStatusCacheStats> scmStatusCacheStats_;
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
inline ScmStatusCacheStats&
EdenStats::getStatsForCurrentThread<ScmStatusCacheStats>() {
  return *scmStatusCacheStats_.get();
}

template <>
inline FakeStats& EdenStats::getStatsForCurrentThread<FakeStats>() {
  return *fakeStats_.get();
}

struct FuseStats : StatsGroup<FuseStats> {
  Duration lookup{"fuse.lookup_us"};
  Counter lookupSuccessful{"fuse.lookup_successful"};
  Counter lookupFailure{"fuse.lookup_failure"};
  Duration forget{"fuse.forget_us"};
  Counter forgetSuccessful{"fuse.forget_successful"};
  Counter forgetFailure{"fuse.forget_failure"};
  Duration getattr{"fuse.getattr_us"};
  Counter getattrSuccessful{"fuse.getattr_successful"};
  Counter getattrFailure{"fuse.getattr_failure"};
  Duration setattr{"fuse.setattr_us"};
  Counter setattrSuccessful{"fuse.setattr_successful"};
  Counter setattrFailure{"fuse.setattr_failure"};
  Duration readlink{"fuse.readlink_us"};
  Counter readlinkSuccessful{"fuse.readlink_successful"};
  Counter readlinkFailure{"fuse.readlink_failure"};
  Duration mknod{"fuse.mknod_us"};
  Counter mknodSuccessful{"fuse.mknod_successful"};
  Counter mknodFailure{"fuse.mknod_failure"};
  Duration mkdir{"fuse.mkdir_us"};
  Counter mkdirSuccessful{"fuse.mkdir_successful"};
  Counter mkdirFailure{"fuse.mkdir_failure"};
  Duration unlink{"fuse.unlink_us"};
  Counter unlinkSuccessful{"fuse.unlink_successful"};
  Counter unlinkFailure{"fuse.unlink_failure"};
  Duration rmdir{"fuse.rmdir_us"};
  Counter rmdirSuccessful{"fuse.rmdir_successful"};
  Counter rmdirFailure{"fuse.rmdir_failure"};
  Duration symlink{"fuse.symlink_us"};
  Counter symlinkSuccessful{"fuse.symlink_successful"};
  Counter symlinkFailure{"fuse.symlink_failure"};
  Duration rename{"fuse.rename_us"};
  Counter renameSuccessful{"fuse.rename_successful"};
  Counter renameFailure{"fuse.rename_failure"};
  Duration link{"fuse.link_us"};
  Counter linkSuccessful{"fuse.link_successful"};
  Counter linkFailure{"fuse.link_failure"};
  Duration open{"fuse.open_us"};
  Counter openSuccessful{"fuse.open_successful"};
  Counter openFailure{"fuse.open_failure"};
  Duration read{"fuse.read_us"};
  Counter readSuccessful{"fuse.read_successful"};
  Counter readFailure{"fuse.read_failure"};
  Duration write{"fuse.write_us"};
  Counter writeSuccessful{"fuse.write_successful"};
  Counter writeFailure{"fuse.write_failure"};
  Duration flush{"fuse.flush_us"};
  Counter flushSuccessful{"fuse.flush_successful"};
  Counter flushFailure{"fuse.flush_failure"};
  Duration release{"fuse.release_us"};
  Counter releaseSuccessful{"fuse.release_successful"};
  Counter releaseFailure{"fuse.release_failure"};
  Duration fsync{"fuse.fsync_us"};
  Counter fsyncSuccessful{"fuse.fsync_successful"};
  Counter fsyncFailure{"fuse.fsync_failure"};
  Duration opendir{"fuse.opendir_us"};
  Counter opendirSuccessful{"fuse.opendir_successful"};
  Counter opendirFailure{"fuse.opendir_failure"};
  Duration readdir{"fuse.readdir_us"};
  Counter readdirSuccessful{"fuse.readdir_successful"};
  Counter readdirFailure{"fuse.readdir_failure"};
  Duration releasedir{"fuse.releasedir_us"};
  Counter releasedirSuccessful{"fuse.releasedir_successful"};
  Counter releasedirFailure{"fuse.releasedir_failure"};
  Duration fsyncdir{"fuse.fsyncdir_us"};
  Counter fsyncdirSuccessful{"fuse.fsyncdir_successful"};
  Counter fsyncdirFailure{"fuse.fsyncdir_failure"};
  Duration statfs{"fuse.statfs_us"};
  Counter statfsSuccessful{"fuse.statfs_successful"};
  Counter statfsFailure{"fuse.statfs_failure"};
  Duration setxattr{"fuse.setxattr_us"};
  Counter setxattrSuccessful{"fuse.setxattr_successful"};
  Counter setxattrFailure{"fuse.setxattr_failure"};
  Duration getxattr{"fuse.getxattr_us"};
  Counter getxattrSuccessful{"fuse.getxattr_successful"};
  Counter getxattrFailure{"fuse.getxattr_failure"};
  Duration listxattr{"fuse.listxattr_us"};
  Counter listxattrSuccessful{"fuse.listxattr_successful"};
  Counter listxattrFailure{"fuse.listxattr_failure"};
  Duration removexattr{"fuse.removexattr_us"};
  Counter removexattrSuccessful{"fuse.removexattr_successful"};
  Counter removexattrFailure{"fuse.removexattr_failure"};
  Duration access{"fuse.access_us"};
  Counter accessSuccessful{"fuse.access_successful"};
  Counter accessFailure{"fuse.access_failure"};
  Duration create{"fuse.create_us"};
  Counter createSuccessful{"fuse.create_successful"};
  Counter createFailure{"fuse.create_failure"};
  Duration bmap{"fuse.bmap_us"};
  Counter bmapSuccessful{"fuse.bmap_successful"};
  Counter bmapFailure{"fuse.bmap_failure"};
  Duration ioctl{"fuse.ioctl_us"};
  Duration poll{"fuse.poll_us"};
  Duration forgetmulti{"fuse.forgetmulti_us"};
  Counter forgetmultiSuccessful{"fuse.forgetmulti_successful"};
  Counter forgetmultiFailure{"fuse.forgetmulti_failure"};
  Duration fallocate{"fuse.fallocate_us"};
  Counter fallocateSuccessful{"fuse.fallocate_successful"};
  Counter fallocateFailure{"fuse.fallocate_failure"};
};

struct NfsStats : StatsGroup<NfsStats> {
  Duration nfsNull{"nfs.null_us"};
  Counter nfsNullSuccessful{"nfs.null_successful"};
  Counter nfsNullFailure{"nfs.null_failure"};
  Duration nfsGetattr{"nfs.getattr_us"};
  Counter nfsGetattrSuccessful{"nfs.getattr_successful"};
  Counter nfsGetattrFailure{"nfs.getattr_failure"};
  Duration nfsSetattr{"nfs.setattr_us"};
  Counter nfsSetattrSuccessful{"nfs.setattr_successful"};
  Counter nfsSetattrFailure{"nfs.setattr_failure"};
  Duration nfsLookup{"nfs.lookup_us"};
  Counter nfsLookupSuccessful{"nfs.lookup_successful"};
  Counter nfsLookupFailure{"nfs.lookup_failure"};
  Duration nfsAccess{"nfs.access_us"};
  Counter nfsAccessSuccessful{"nfs.access_successful"};
  Counter nfsAccessFailure{"nfs.access_failure"};
  Duration nfsReadlink{"nfs.readlink_us"};
  Counter nfsReadlinkSuccessful{"nfs.readlink_successful"};
  Counter nfsReadlinkFailure{"nfs.readlink_failure"};
  Duration nfsRead{"nfs.read_us"};
  Counter nfsReadSuccessful{"nfs.read_successful"};
  Counter nfsReadFailure{"nfs.read_failure"};
  Duration nfsWrite{"nfs.write_us"};
  Counter nfsWriteSuccessful{"nfs.write_successful"};
  Counter nfsWriteFailure{"nfs.write_failure"};
  Duration nfsCreate{"nfs.create_us"};
  Counter nfsCreateSuccessful{"nfs.create_successful"};
  Counter nfsCreateFailure{"nfs.create_failure"};
  Duration nfsMkdir{"nfs.mkdir_us"};
  Counter nfsMkdirSuccessful{"nfs.mkdir_successful"};
  Counter nfsMkdirFailure{"nfs.mkdir_failure"};
  Duration nfsSymlink{"nfs.symlink_us"};
  Counter nfsSymlinkSuccessful{"nfs.symlink_successful"};
  Counter nfsSymlinkFailure{"nfs.symlink_failure"};
  Duration nfsMknod{"nfs.mknod_us"};
  Counter nfsMknodSuccessful{"nfs.mknod_successful"};
  Counter nfsMknodFailure{"nfs.mknod_failure"};
  Duration nfsRemove{"nfs.remove_us"};
  Counter nfsRemoveSuccessful{"nfs.remove_successful"};
  Counter nfsRemoveFailure{"nfs.remove_failure"};
  Duration nfsRmdir{"nfs.rmdir_us"};
  Counter nfsRmdirSuccessful{"nfs.rmdir_successful"};
  Counter nfsRmdirFailure{"nfs.rmdir_failure"};
  Duration nfsRename{"nfs.rename_us"};
  Counter nfsRenameSuccessful{"nfs.rename_successful"};
  Counter nfsRenameFailure{"nfs.rename_failure"};
  Duration nfsLink{"nfs.link_us"};
  Counter nfsLinkSuccessful{"nfs.link_successful"};
  Counter nfsLinkFailure{"nfs.link_failure"};
  Duration nfsReaddir{"nfs.readdir_us"};
  Counter nfsReaddirSuccessful{"nfs.readdir_successful"};
  Counter nfsReaddirFailure{"nfs.readdir_failure"};
  Duration nfsReaddirplus{"nfs.readdirplus_us"};
  Counter nfsReaddirplusSuccessful{"nfs.readdirplus_successful"};
  Counter nfsReaddirplusFailure{"nfs.readdirplus_failure"};
  Duration nfsFsstat{"nfs.fsstat_us"};
  Counter nfsFsstatSuccessful{"nfs.fsstat_successful"};
  Counter nfsFsstatFailure{"nfs.fsstat_failure"};
  Duration nfsFsinfo{"nfs.fsinfo_us"};
  Counter nfsFsinfoSuccessful{"nfs.fsinfo_successful"};
  Counter nfsFsinfoFailure{"nfs.fsinfo_failure"};
  Duration nfsPathconf{"nfs.pathconf_us"};
  Counter nfsPathconfSuccessful{"nfs.pathconf_successful"};
  Counter nfsPathconfFailure{"nfs.pathconf_failure"};
  Duration nfsCommit{"nfs.commit_us"};
  Counter nfsCommitSuccessful{"nfs.commit_successful"};
  Counter nfsCommitFailure{"nfs.commit_failure"};
};

struct PrjfsStats : StatsGroup<PrjfsStats> {
  Counter outOfOrderCreate{"prjfs.out_of_order_create"};
  Duration queuedFileNotification{"prjfs.queued_file_notification_us"};
  Duration filesystemSync{"prjfs.filesystem_sync_us"};
  Counter filesystemSyncSuccessful{"prjfs.filesystem_sync_successful"};
  Counter filesystemSyncFailure{"prjfs.filesystem_sync_failure"};

  Duration newFileCreated{"prjfs.newFileCreated_us"};
  Counter newFileCreatedSuccessful{"prjfs.newFileCreated_successful"};
  Counter newFileCreatedFailure{"prjfs.newFileCreated_failure"};
  Duration fileOverwritten{"prjfs.fileOverwritten_us"};
  Counter fileOverwrittenSuccessful{"prjfs.fileOverwritten_successful"};
  Counter fileOverwrittenFailure{"prjfs.fileOverwritten_failure"};

  Duration fileHandleClosedFileModified{
      "prjfs.fileHandleClosedFileModified_us"};
  Counter fileHandleClosedFileModifiedSuccessful{
      "prjfs.fileHandleClosedFileModified_successful"};
  Counter fileHandleClosedFileModifiedFailure{
      "prjfs.fileHandleClosedFileModified_failure"};
  Duration fileRenamed{"prjfs.fileRenamed_us"};
  Counter fileRenamedSuccessful{"prjfs.fileRenamed_successful"};
  Counter fileRenamedFailure{"prjfs.fileRenamed_failure"};
  Duration preDelete{"prjfs.preDelete_us"};
  Counter preDeleteSuccessful{"prjfs.preDelete_successful"};
  Counter preDeleteFailure{"prjfs.preDelete_failure"};
  Duration preRenamed{"prjfs.preRenamed_us"};
  Counter preRenamedSuccessful{"prjfs.preRenamed_successful"};
  Counter preRenamedFailure{"prjfs.preRenamed_failure"};
  Duration fileHandleClosedFileDeleted{"prjfs.fileHandleClosedFileDeleted_us"};
  Counter fileHandleClosedFileDeletedSuccessful{
      "prjfs.fileHandleClosedFileDeleted_successful"};
  Counter fileHandleClosedFileDeletedFailure{
      "prjfs.fileHandleClosedFileDeleted_failure"};
  Duration preSetHardlink{"prjfs.preSetHardlink_us"};
  Counter preSetHardlinkSuccessful{"prjfs.preSetHardlink_successful"};
  Counter preSetHardlinkFailure{"prjfs.preSetHardlink_failure"};
  Duration preConvertToFull{"prjfs.preConvertToFull_us"};
  Counter preConvertToFullSuccessful{"prjfs.preConvertToFull_successful"};
  Counter preConvertToFullFailure{"prjfs.preConvertToFull_failure"};

  Duration openDir{"prjfs.opendir_us"};
  Counter openDirSuccessful{"prjfs.opendir_successful"};
  Counter openDirFailure{"prjfs.opendir_failure"};
  Duration readDir{"prjfs.readdir_us"};
  Counter readDirSuccessful{"prjfs.readdir_successful"};
  Counter readDirFailure{"prjfs.readdir_failure"};
  Duration lookup{"prjfs.lookup_us"};
  Counter lookupSuccessful{"prjfs.lookup_successful"};
  Counter lookupFailure{"prjfs.lookup_failure"};
  Duration access{"prjfs.access_us"};
  Counter accessSuccessful{"prjfs.access_successful"};
  Counter accessFailure{"prjfs.access_failure"};
  Duration read{"prjfs.read_us"};
  Counter readSuccessful{"prjfs.read_successful"};
  Counter readFailure{"prjfs.read_failure"};

  Duration removeCachedFile{"prjfs.remove_cached_file_us"};
  Counter removeCachedFileSuccessful{"prjfs.remove_cached_file_successful"};
  Counter removeCachedFileFailure{"prjfs.remove_cached_file_failure"};
  Duration addDirectoryPlaceholder{"prjfs.add_directory_placeholder_us"};
  Counter addDirectoryPlaceholderSuccessful{
      "prjfs.add_directory_placeholder_successful"};
  Counter addDirectoryPlaceholderFailure{
      "prjfs.add_directory_placeholder_failure"};
};

/**
 * @see ObjectStore
 */
struct ObjectStoreStats : StatsGroup<ObjectStoreStats> {
  Duration getTree{"store.get_tree_us"};
  Duration getTreeMemoryDuration{"store.get_tree.memory_us"};
  Duration getTreeLocalstoreDuration{"store.get_tree.localstore_us"};
  Duration getTreeBackingstoreDuration{"store.get_tree.backingstore_us"};
  Duration getTreeMetadata{"store.get_tree_metadata_us"};
  Duration getBlob{"store.get_blob_us"};
  Duration getBlobMetadata{"store.get_blob_metadata_us"};
  Duration getBlobMetadataMemoryDuration{"store.get_blob_metadata.memory_us"};
  Duration getBlobMetadataLocalstoreDuration{
      "store.get_blob_metadata.localstore_us"};
  Duration getBlobMetadataBackingstoreDuration{
      "store.get_blob_metadata.backingstore_us"};
  Duration getBlobMetadataFromBlobDuration{
      "store.get_blob_metadata.from_blob_us"};
  Duration getRootTree{"store.get_root_tree_us"};

  Counter getBlobFromMemory{"object_store.get_blob.memory"};
  Counter getBlobFromLocalStore{"object_store.get_blob.local_store"};
  Counter getBlobFromBackingStore{"object_store.get_blob.backing_store"};
  Counter getBlobFailed{"object_store.get_blob_failed"};

  Counter getTreeFromMemory{"object_store.get_tree.memory"};
  Counter getTreeFromLocalStore{"object_store.get_tree.local_store"};
  Counter getTreeFromBackingStore{"object_store.get_tree.backing_store"};
  Counter getTreeFailed{"object_store.get_tree_failed"};

  Counter getTreeMetadataFromMemory{"object_store.get_tree_metadata.memory"};
  Counter getTreeMetadataFromBackingStore{
      "object_store.get_tree_metadata.backing_store"};
  Counter getTreeMetadataFailed{"object_store.get_tree_metadata_failed"};

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
  Duration getTreeMetadata{"store.sapling.get_tree_metadata_us"};
  Duration fetchTreeMetadata{"store.sapling.fetch_tree_metadata_us"};
  Counter fetchTreeMetadataLocal{"store.sapling.fetch_tree_metadata_local"};
  Counter fetchTreeMetadataRemote{"store.sapling.fetch_tree_metadata_remote"};
  Counter fetchTreeMetadataSuccess{"store.sapling.fetch_tree_metadata_success"};
  Counter fetchTreeMetadataFailure{"store.sapling.fetch_tree_metadata_failure"};
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
  Counter fetchGlobFilesSuccess{"store.sapling.fetch_glob_files_success"};
  Counter fetchGlobFilesFailure{"store.sapling.fetch_glob_files_failure"};
  Duration fetchGlobFiles{"store.sapling.fetch_glob_files_us"};
  Counter loadProxyHash{"store.sapling.load_proxy_hash"};
};

struct JournalStats : StatsGroup<JournalStats> {
  Counter truncatedReads{"journal.truncated_reads"};
  Counter filesAccumulated{"journal.files_accumulated"};
  Counter journalStatusCacheHit{"journal.status_cache_hit"};
  Counter journalStatusCachePend{"journal.status_cache_pend"};
  Counter journalStatusCacheMiss{"journal.status_cache_miss"};
  Counter journalStatusCacheSkip{"journal.status_cache_skip"};
  Duration accumulateRange{"journal.accumulate_range_us"};
};

struct ThriftStats : StatsGroup<ThriftStats> {
  Duration streamChangesSince{
      "thrift.StreamingEdenService.streamChangesSince.streaming_time_us"};

  Duration streamSelectedChangesSince{
      "thrift.StreamingEdenService.streamSelectedChangesSince.streaming_time_us"};

  Counter globFilesSaplingRemoteAPISuccess{
      "thrift.EdenServiceHandler.glob_files.sapling_remote_api_success"};
  Counter globFilesSaplingRemoteAPIFallback{
      "thrift.EdenServiceHandler.glob_files.sapling_remote_api_fallback"};
  Counter globFilesLocal{"thrift.EdenServiceHandler.glob_files.local_success"};
  Duration globFilesSaplingRemoteAPISuccessDuration{
      "thrift.EdenServiceHandler.glob_files.sapling_remote_api_success_duration_us"};
  Duration globFilesSaplingRemoteAPIFallbackDuration{
      "thrift.EdenServiceHandler.glob_files.sapling_remote_api_fallback_duration_us"};
  Duration globFilesLocalDuration{
      "thrift.EdenServiceHandler.glob_files.local_duration_us"};
  Duration globFilesLocalOffloadableDuration{
      "thrift.EdenServiceHandler.glob_files.local_offloadable_duration_us"};
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

struct ScmStatusCacheStats : StatsGroup<TreeCacheStats> {
  Counter getHit{"scm_status_cache.get_hit"};
  Counter getMiss{"scm_status_cache.get_miss"};
  Counter insertEviction{"scm_status_cache.insert_eviction"};
  Counter objectDrop{"scm_status_cache.object_drop"};
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
