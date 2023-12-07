/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <memory>

#include <fb303/detail/QuantileStatWrappers.h>
#include <folly/ThreadLocal.h>
#include <folly/stop_watch.h>

#include "eden/fs/eden-config.h"
#include "eden/fs/utils/RefPtr.h"

namespace facebook::eden {

struct FuseStats;
struct NfsStats;
struct PrjfsStats;
struct ObjectStoreStats;
struct LocalStoreStats;
struct HgBackingStoreStats;
struct HgImporterStats;
struct JournalStats;
struct ThriftStats;
struct TelemetryStats;
struct OverlayStats;
struct InodeMapStats;
struct InodeMetadataTableStats;

/**
 * StatsGroupBase is a base class for a group of thread-local stats
 * structures.
 *
 * Each StatsGroupBase object should only be used from a single thread. The
 * EdenStats object should be used to maintain one StatsGroupBase object
 * for each thread that needs to access/update the stats.
 */
class StatsGroupBase {
  using Stat = fb303::detail::QuantileStatWrapper;

 public:
  /**
   * Counter is used to record events.
   */
  class Counter : private Stat {
   public:
    explicit Counter(std::string_view name);

    using Stat::addValue;
  };

  /**
   * Duration is used for stats that measure elapsed times.
   *
   * In general, EdenFS measures latencies in units of microseconds.
   * Duration enforces that its stat names end in "_us".
   */
  class Duration : private Stat {
   public:
    explicit Duration(std::string_view name);

    /**
     * Record a duration in microseconds to the QuantileStatWrapper. Also
     * increments the .count statistic.
     */
    template <typename Rep, typename Period>
    void addDuration(std::chrono::duration<Rep, Period> elapsed) {
      // TODO: Implement a general overflow check when converting from seconds
      // or milliseconds to microseconds. Fortunately, this use case deals with
      // short durations.
      addDuration(
          std::chrono::duration_cast<std::chrono::microseconds>(elapsed));
    }

    void addDuration(std::chrono::microseconds elapsed);
  };
};

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
  ThreadLocal<HgBackingStoreStats> hgBackingStoreStats_;
  ThreadLocal<HgImporterStats> hgImporterStats_;
  ThreadLocal<JournalStats> journalStats_;
  ThreadLocal<ThriftStats> thriftStats_;
  ThreadLocal<TelemetryStats> telemetryStats_;
  ThreadLocal<OverlayStats> overlayStats_;
  ThreadLocal<InodeMapStats> inodeMapStats_;
  ThreadLocal<InodeMetadataTableStats> inodeMetadataTableStats_;
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
inline HgBackingStoreStats&
EdenStats::getStatsForCurrentThread<HgBackingStoreStats>() {
  return *hgBackingStoreStats_.get();
}

template <>
inline HgImporterStats& EdenStats::getStatsForCurrentThread<HgImporterStats>() {
  return *hgImporterStats_.get();
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

template <typename T>
class StatsGroup : public StatsGroupBase {
 public:
  /**
   * Statistics are often updated on a thread separate from the thread that
   * started a request. Since stat objects are thread-local, we cannot hold
   * pointers directly to them. Instead, we store a pointer-to-member and look
   * up the calling thread's object.
   */
  using DurationPtr = Duration T::*;
};

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

  Counter getBlobFromMemory{"object_store.get_blob.memory"};
  Counter getBlobFromLocalStore{"object_store.get_blob.local_store"};
  Counter getBlobFromBackingStore{"object_store.get_blob.backing_store"};

  Counter getTreeFromMemory{"object_store.get_tree.memory"};
  Counter getTreeFromLocalStore{"object_store.get_tree.local_store"};
  Counter getTreeFromBackingStore{"object_store.get_tree.backing_store"};

  Counter getBlobMetadataFromMemory{"object_store.get_blob_metadata.memory"};
  Counter getBlobMetadataFromLocalStore{
      "object_store.get_blob_metadata.local_store"};
  Counter getBlobMetadataFromBackingStore{
      "object_store.get_blob_metadata.backing_store"};
  Counter getLocalBlobMetadataFromBackingStore{
      "object_store.get_blob_metadata.backing_store_cache"};
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
 * @see HgBackingStore
 *
 * Terminology:
 *   get = entire lookup process, including both Sapling disk hits and fetches
 *   fetch = includes asynchronous retrieval from Mononoke
 *   import = fall back on hg debugedenimporthelper process
 */
struct HgBackingStoreStats : StatsGroup<HgBackingStoreStats> {
  Duration getTree{"store.hg.get_tree_us"};
  Duration fetchTree{"store.hg.fetch_tree_us"};
  Counter fetchTreeRetrySuccess{"store.hg.fetch_tree_retry_success"};
  Counter fetchTreeRetryFailure{"store.hg.fetch_tree_retry_failure"};
  Duration importTreeDuration{"store.hg.import_tree_us"};
  Counter importTreeSuccess{"store.hg.import_tree_success"};
  Counter importTreeFailure{"store.hg.import_tree_failure"};
  Counter importTreeError{"store.hg.import_tree_error"};
  Duration getBlob{"store.hg.get_blob_us"};
  Duration fetchBlob{"store.hg.fetch_blob_us"};
  Counter fetchBlobRetrySuccess{"store.hg.fetch_blob_retry_success"};
  Counter fetchBlobRetryFailure{"store.hg.fetch_blob_retry_failure"};
  Duration importBlobDuration{"store.hg.import_blob_us"};
  Counter importBlobSuccess{"store.hg.import_blob_success"};
  Counter importBlobFailure{"store.hg.import_blob_failure"};
  Counter importBlobError{"store.hg.import_blob_error"};
  Duration getBlobMetadata{"store.hg.get_blob_metadata_us"};
  Duration fetchBlobMetadata{"store.hg.fetch_blob_metadata_us"};
  Counter loadProxyHash{"store.hg.load_proxy_hash"};
};

/**
 * @see HgImporter
 * @see HgBackingStore
 */
struct HgImporterStats : StatsGroup<HgImporterStats> {
  Counter catFile{"hg_importer.cat_file"};
  Counter fetchTree{"hg_importer.fetch_tree"};
  Counter manifest{"hg_importer.manifest"};
  Counter manifestNodeForCommit{"hg_importer.manifest_node_for_commit"};
  Counter prefetchFiles{"hg_importer.prefetch_files"};
};

struct JournalStats : StatsGroup<JournalStats> {
  Counter truncatedReads{"journal.truncated_reads"};
  Counter filesAccumulated{"journal.files_accumulated"};
};

struct ThriftStats : StatsGroup<ThriftStats> {
  Duration streamChangesSince{
      "thrift.StreamingEdenService.streamChangesSince.streaming_time_us"};

  Duration streamSelectedChangesSince{
      "thrift.StreamingEdenService.streamSelectedChangesSince.streaming_time_us"};
};

struct TelemetryStats : StatsGroup<TelemetryStats> {
  Counter subprocessLoggerFailure{"telemetry.subprocess_logger_failure"};
};

struct OverlayStats : StatsGroup<OverlayStats> {
  Duration saveOverlayDir{"overlay.save_overlay_dir_us"};
  Duration loadOverlayDir{"overlay.load_overlay_dir_us"};
  Duration removeOverlayFile{"overlay.remove_overlay_file_us"};
  Duration removeOverlayDir{"overlay.remove_overlay_dir_us"};
  Duration hasOverlayDir{"overlay.has_overlay_dir_us"};
  Duration hasOverlayFile{"overlay.has_overlay_file_us"};
  Duration addChild{"overlay.add_child_us"};
  Duration removeChild{"overlay.remove_child_us"};
  Duration removeChildren{"overlay.remove_children_us"};
  Duration renameChild{"overlay.rename_child_us"};
  Counter loadOverlayDirHit{"overlay.load_overlay_dir_hit"};
  Counter loadOverlayDirMiss{"overlay.load_overlay_dir_miss"};
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

/**
 * On construction, notes the current time. On destruction, records the elapsed
 * time in the specified EdenStats Duration.
 *
 * Moveable, but not copyable.
 */
class DurationScope {
 public:
  DurationScope() = delete;

  template <typename T>
  DurationScope(EdenStatsPtr&& edenStats, StatsGroupBase::Duration T::*duration)
      : edenStats_{std::move(edenStats)},
        // This use of std::function won't allocate on libstdc++,
        // libc++, or Microsoft STL. All three have a couple pointers
        // worth of small buffer inline storage.
        updateScope_{[duration](EdenStats& stats, StopWatch::duration elapsed) {
          stats.addDuration(duration, elapsed);
        }} {
    assert(edenStats_);
  }

  template <typename T>
  DurationScope(
      const EdenStatsPtr& edenStats,
      StatsGroupBase::Duration T::*duration)
      : DurationScope{edenStats.copy(), duration} {}

  ~DurationScope() noexcept;

  DurationScope(DurationScope&& that) = default;
  DurationScope& operator=(DurationScope&& that) = default;

  DurationScope(const DurationScope&) = delete;
  DurationScope& operator=(const DurationScope&) = delete;

 private:
  using StopWatch = folly::stop_watch<>;
  StopWatch stopWatch_;
  EdenStatsPtr edenStats_;
  std::function<void(EdenStats& stats, StopWatch::duration)> updateScope_;
};

} // namespace facebook::eden
