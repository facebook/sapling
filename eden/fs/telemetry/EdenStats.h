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

class FsChannelThreadStats;
class ObjectStoreThreadStats;
class HgBackingStoreThreadStats;
class HgImporterThreadStats;
class JournalThreadStats;
class ThriftThreadStats;

class EdenStats {
 public:
  /**
   * This function can be called on any thread.
   *
   * The returned object can be used only on the current thread.
   */
  FsChannelThreadStats& getFsChannelStatsForCurrentThread();

  /**
   * This function can be called on any thread.
   *
   * The returned object can be used only on the current thread.
   */
  ObjectStoreThreadStats& getObjectStoreStatsForCurrentThread();

  /**
   * This function can be called on any thread.
   *
   * The returned object can be used only on the current thread.
   */
  HgBackingStoreThreadStats& getHgBackingStoreStatsForCurrentThread();

  /**
   * This function can be called on any thread.
   *
   * The returned object can be used only on the current thread.
   */
  HgImporterThreadStats& getHgImporterStatsForCurrentThread();

  /**
   * This function can be called on any thread.
   *
   * The returned object can be used only on the current thread.
   */
  JournalThreadStats& getJournalStatsForCurrentThread();

  /**
   * This function can be called on any thread.
   *
   * The returned object can be used only on the current thread.
   */
  ThriftThreadStats& getThriftStatsForCurrentThread();

  /**
   * Returns a thread-local stats group.
   *
   * The returned object must only be used on the current thread.
   */
  template <typename T>
  T& getStatsForCurrentThread() = delete;

  /**
   * This function can be called on any thread.
   */
  void flush();

 private:
  class ThreadLocalTag {};

  folly::ThreadLocal<FsChannelThreadStats, ThreadLocalTag, void>
      threadLocalFsChannelStats_;
  folly::ThreadLocal<ObjectStoreThreadStats, ThreadLocalTag, void>
      threadLocalObjectStoreStats_;
  folly::ThreadLocal<HgBackingStoreThreadStats, ThreadLocalTag, void>
      threadLocalHgBackingStoreStats_;
  folly::ThreadLocal<HgImporterThreadStats, ThreadLocalTag, void>
      threadLocalHgImporterStats_;
  folly::ThreadLocal<JournalThreadStats, ThreadLocalTag, void>
      threadLocalJournalStats_;
  folly::ThreadLocal<ThriftThreadStats, ThreadLocalTag, void>
      threadLocalThriftStats_;
};

template <>
inline FsChannelThreadStats&
EdenStats::getStatsForCurrentThread<FsChannelThreadStats>() {
  return *threadLocalFsChannelStats_.get();
}

template <>
inline ObjectStoreThreadStats&
EdenStats::getStatsForCurrentThread<ObjectStoreThreadStats>() {
  return *threadLocalObjectStoreStats_.get();
}

template <>
inline HgBackingStoreThreadStats&
EdenStats::getStatsForCurrentThread<HgBackingStoreThreadStats>() {
  return *threadLocalHgBackingStoreStats_.get();
}

template <>
inline HgImporterThreadStats&
EdenStats::getStatsForCurrentThread<HgImporterThreadStats>() {
  return *threadLocalHgImporterStats_.get();
}

template <>
inline JournalThreadStats&
EdenStats::getStatsForCurrentThread<JournalThreadStats>() {
  return *threadLocalJournalStats_.get();
}

template <>
inline ThriftThreadStats&
EdenStats::getStatsForCurrentThread<ThriftThreadStats>() {
  return *threadLocalThriftStats_.get();
}

/**
 * EdenThreadStatsBase is a base class for a group of thread-local stats
 * structures.
 *
 * Each EdenThreadStatsBase object should only be used from a single thread. The
 * EdenStats object should be used to maintain one EdenThreadStatsBase object
 * for each thread that needs to access/update the stats.
 */
class EdenThreadStatsBase {
 protected:
  // TODO: make this private when ActivityRecorder uses Duration instead.
  using Stat = fb303::detail::QuantileStatWrapper;

 public:
  /**
   * Counter is used to record events.
   */
  using Counter = Stat;

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

 protected:
  Stat createStat(std::string_view name);
};

template <typename T>
class EdenThreadStats : public EdenThreadStatsBase {
 public:
  /**
   * Statistics are often updated on a thread separate from the thread that
   * started a request. Since stat objects are thread-local, we cannot hold
   * pointers directly to them. Instead, we store a pointer-to-member.
   */
  using DurationPtr = Duration T::*;
};

class FsChannelThreadStats : public EdenThreadStats<FsChannelThreadStats> {
 public:
#ifndef _WIN32
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
#else
  Counter outOfOrderCreate{createStat("prjfs.out_of_order_create")};
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
#endif
};

/**
 * @see ObjectStore
 */
class ObjectStoreThreadStats : public EdenThreadStats<ObjectStoreThreadStats> {
 public:
  Duration getTree{"store.get_tree_us"};
  Duration getBlob{"store.get_blob_us"};
  Duration getBlobMetadata{"store.get_blob_metadata_us"};

  Counter getBlobFromLocalStore{
      createStat("object_store.get_blob.local_store")};
  Counter getBlobFromBackingStore{
      createStat("object_store.get_blob.backing_store")};

  Counter getBlobMetadataFromMemory{
      createStat("object_store.get_blob_metadata.memory")};
  Counter getBlobMetadataFromLocalStore{
      createStat("object_store.get_blob_metadata.local_store")};
  Counter getBlobMetadataFromBackingStore{
      createStat("object_store.get_blob_metadata.backing_store")};
  Counter getLocalBlobMetadataFromBackingStore{
      createStat("object_store.get_blob_metadata.backing_store_cache")};

  Counter getBlobSizeFromLocalStore{
      createStat("object_store.get_blob_size.local_store")};
  Counter getBlobSizeFromBackingStore{
      createStat("object_store.get_blob_size.backing_store")};
};

/**
 * @see HgBackingStore
 */
class HgBackingStoreThreadStats
    : public EdenThreadStats<HgBackingStoreThreadStats> {
 public:
  Duration hgBackingStoreGetBlob{"store.hg.get_blob_us"};
  Duration hgBackingStoreImportBlob{"store.hg.import_blob_us"};
  Duration hgBackingStoreGetTree{"store.hg.get_tree_us"};
  Duration hgBackingStoreImportTree{"store.hg.import_tree_us"};
  Duration hgBackingStoreGetBlobMetadata{"store.hg.get_blob_metadata_us"};
};

/**
 * @see HgImporter
 * @see HgBackingStore
 */
class HgImporterThreadStats : public EdenThreadStats<HgImporterThreadStats> {
 public:
  Counter catFile{createStat("hg_importer.cat_file")};
  Counter fetchTree{createStat("hg_importer.fetch_tree")};
  Counter manifest{createStat("hg_importer.manifest")};
  Counter manifestNodeForCommit{
      createStat("hg_importer.manifest_node_for_commit")};
  Counter prefetchFiles{createStat("hg_importer.prefetch_files")};
};

class JournalThreadStats : public EdenThreadStats<JournalThreadStats> {
 public:
  Counter truncatedReads{createStat("journal.truncated_reads")};
  Counter filesAccumulated{createStat("journal.files_accumulated")};
};

class ThriftThreadStats : public EdenThreadStats<ThriftThreadStats> {
 public:
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
      EdenThreadStatsBase::Duration T::*duration)
      : edenStats_{std::move(edenStats)},
        // This use of std::function won't allocate on libstdc++,
        // libc++, or Microsoft STL. All three have a couple pointers
        // worth of small buffer inline storage.
        updateScope_{[duration](EdenStats& stats, StopWatch::duration elapsed) {
          (stats.getStatsForCurrentThread<T>().*duration).addDuration(elapsed);
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
