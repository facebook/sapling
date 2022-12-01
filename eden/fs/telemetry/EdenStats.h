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

namespace facebook::eden {

struct FuseStats;
struct NfsStats;
struct PrjfsStats;
struct ObjectStoreStats;
struct HgBackingStoreStats;
struct HgImporterStats;
struct JournalStats;
struct ThriftStats;

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

class EdenStats {
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
  ThreadLocal<HgBackingStoreStats> hgBackingStoreStats_;
  ThreadLocal<HgImporterStats> hgImporterStats_;
  ThreadLocal<JournalStats> journalStats_;
  ThreadLocal<ThriftStats> thriftStats_;
};

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

  Duration newFileCreated{"prjfs.newFileCreated_us"};
  Duration fileOverwritten{"prjfs.fileOverwritten_us"};
  Duration fileHandleClosedFileModified{
      "prjfs.fileHandleClosedFileModified_us"};
  Duration fileRenamed{"prjfs.fileRenamed_us"};
  Duration preDelete{"prjfs.preDelete_us"};
  Duration preRenamed{"prjfs.preRenamed_us"};
  Duration fileHandleClosedFileDeleted{"prjfs.fileHandleClosedFileDeleted_us"};
  Duration preSetHardlink{"prjfs.preSetHardlink_us"};

  Duration openDir{"prjfs.opendir_us"};
  Duration readDir{"prjfs.readdir_us"};
  Duration lookup{"prjfs.lookup_us"};
  Duration access{"prjfs.access_us"};
  Duration read{"prjfs.read_us"};
};

/**
 * @see ObjectStore
 */
struct ObjectStoreStats : StatsGroup<ObjectStoreStats> {
  Duration getTree{"store.get_tree_us"};
  Duration getBlob{"store.get_blob_us"};
  Duration getBlobMetadata{"store.get_blob_metadata_us"};

  Counter getBlobFromLocalStore{"object_store.get_blob.local_store"};
  Counter getBlobFromBackingStore{"object_store.get_blob.backing_store"};

  Counter getBlobMetadataFromMemory{"object_store.get_blob_metadata.memory"};
  Counter getBlobMetadataFromLocalStore{
      "object_store.get_blob_metadata.local_store"};
  Counter getBlobMetadataFromBackingStore{
      "object_store.get_blob_metadata.backing_store"};
  Counter getLocalBlobMetadataFromBackingStore{
      "object_store.get_blob_metadata.backing_store_cache"};

  Counter getBlobSizeFromLocalStore{"object_store.get_blob_size.local_store"};
  Counter getBlobSizeFromBackingStore{
      "object_store.get_blob_size.backing_store"};
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
  Duration importTree{"store.hg.import_tree_us"};
  Duration getBlob{"store.hg.get_blob_us"};
  Duration fetchBlob{"store.hg.fetch_blob_us"};
  Duration importBlob{"store.hg.import_blob_us"};
  Duration getBlobMetadata{"store.hg.get_blob_metadata_us"};
  Counter loadProxyHash{"store.hg.load_proxy_hash"};
  Counter auxMetadataMiss{"store.hg.aux_metadata_miss"};
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
  DurationScope(
      std::shared_ptr<EdenStats> edenStats,
      StatsGroupBase::Duration T::*duration)
      : edenStats_{std::move(edenStats)},
        // This use of std::function won't allocate on libstdc++,
        // libc++, or Microsoft STL. All three have a couple pointers
        // worth of small buffer inline storage.
        updateScope_{[duration](EdenStats& stats, StopWatch::duration elapsed) {
          stats.addDuration(duration, elapsed);
        }} {
    assert(edenStats_);
  }

  ~DurationScope() noexcept;

  DurationScope(DurationScope&& that) = default;
  DurationScope& operator=(DurationScope&& that) = default;

  DurationScope(const DurationScope&) = delete;
  DurationScope& operator=(const DurationScope&) = delete;

 private:
  using StopWatch = folly::stop_watch<>;
  StopWatch stopWatch_;
  std::shared_ptr<EdenStats> edenStats_;
  std::function<void(EdenStats& stats, StopWatch::duration)> updateScope_;
};

} // namespace facebook::eden
