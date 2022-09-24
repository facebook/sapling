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

#include "eden/fs/eden-config.h"

namespace facebook::eden {

class ChannelThreadStats;
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
  ChannelThreadStats& getChannelStatsForCurrentThread();

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
   * This function can be called on any thread.
   */
  void flush();

 private:
  class ThreadLocalTag {};

  folly::ThreadLocal<ChannelThreadStats, ThreadLocalTag, void>
      threadLocalChannelStats_;
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

std::shared_ptr<HgImporterThreadStats> getSharedHgImporterStatsForCurrentThread(
    std::shared_ptr<EdenStats>);

/**
 * EdenThreadStatsBase is a base class for a group of thread-local stats
 * structures.
 *
 * Each EdenThreadStatsBase object should only be used from a single thread. The
 * EdenStats object should be used to maintain one EdenThreadStatsBase object
 * for each thread that needs to access/update the stats.
 */
class EdenThreadStatsBase {
 public:
  using Stat = fb303::detail::QuantileStatWrapper;

  class DurationStat : private Stat {
   public:
    explicit DurationStat(std::string_view name);

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

class ChannelThreadStats : public EdenThreadStatsBase {
 public:
  // We track latency in units of microseconds, hence the _us suffix in the
  // stat names below.

#ifndef _WIN32
  DurationStat lookup{"fuse.lookup_us"};
  DurationStat forget{"fuse.forget_us"};
  DurationStat getattr{"fuse.getattr_us"};
  DurationStat setattr{"fuse.setattr_us"};
  DurationStat readlink{"fuse.readlink_us"};
  DurationStat mknod{"fuse.mknod_us"};
  DurationStat mkdir{"fuse.mkdir_us"};
  DurationStat unlink{"fuse.unlink_us"};
  DurationStat rmdir{"fuse.rmdir_us"};
  DurationStat symlink{"fuse.symlink_us"};
  DurationStat rename{"fuse.rename_us"};
  DurationStat link{"fuse.link_us"};
  DurationStat open{"fuse.open_us"};
  DurationStat read{"fuse.read_us"};
  DurationStat write{"fuse.write_us"};
  DurationStat flush{"fuse.flush_us"};
  DurationStat release{"fuse.release_us"};
  DurationStat fsync{"fuse.fsync_us"};
  DurationStat opendir{"fuse.opendir_us"};
  DurationStat readdir{"fuse.readdir_us"};
  DurationStat releasedir{"fuse.releasedir_us"};
  DurationStat fsyncdir{"fuse.fsyncdir_us"};
  DurationStat statfs{"fuse.statfs_us"};
  DurationStat setxattr{"fuse.setxattr_us"};
  DurationStat getxattr{"fuse.getxattr_us"};
  DurationStat listxattr{"fuse.listxattr_us"};
  DurationStat removexattr{"fuse.removexattr_us"};
  DurationStat access{"fuse.access_us"};
  DurationStat create{"fuse.create_us"};
  DurationStat bmap{"fuse.bmap_us"};
  DurationStat ioctl{"fuse.ioctl_us"};
  DurationStat poll{"fuse.poll_us"};
  DurationStat forgetmulti{"fuse.forgetmulti_us"};
  DurationStat fallocate{"fuse.fallocate_us"};

  DurationStat nfsNull{"nfs.null_us"};
  DurationStat nfsGetattr{"nfs.getattr_us"};
  DurationStat nfsSetattr{"nfs.setattr_us"};
  DurationStat nfsLookup{"nfs.lookup_us"};
  DurationStat nfsAccess{"nfs.access_us"};
  DurationStat nfsReadlink{"nfs.readlink_us"};
  DurationStat nfsRead{"nfs.read_us"};
  DurationStat nfsWrite{"nfs.write_us"};
  DurationStat nfsCreate{"nfs.create_us"};
  DurationStat nfsMkdir{"nfs.mkdir_us"};
  DurationStat nfsSymlink{"nfs.symlink_us"};
  DurationStat nfsMknod{"nfs.mknod_us"};
  DurationStat nfsRemove{"nfs.remove_us"};
  DurationStat nfsRmdir{"nfs.rmdir_us"};
  DurationStat nfsRename{"nfs.rename_us"};
  DurationStat nfsLink{"nfs.link_us"};
  DurationStat nfsReaddir{"nfs.readdir_us"};
  DurationStat nfsReaddirplus{"nfs.readdirplus_us"};
  DurationStat nfsFsstat{"nfs.fsstat_us"};
  DurationStat nfsFsinfo{"nfs.fsinfo_us"};
  DurationStat nfsPathconf{"nfs.pathconf_us"};
  DurationStat nfsCommit{"nfs.commit_us"};
#else
  Stat outOfOrderCreate{createStat("prjfs.out_of_order_create")};
  DurationStat queuedFileNotification{"prjfs.queued_file_notification_us"};

  DurationStat newFileCreated{"prjfs.newFileCreated_us"};
  DurationStat fileOverwritten{"prjfs.fileOverwritten_us"};
  DurationStat fileHandleClosedFileModified{
      "prjfs.fileHandleClosedFileModified_us"};
  DurationStat fileRenamed{"prjfs.fileRenamed_us"};
  DurationStat preDelete{"prjfs.preDelete_us"};
  DurationStat preRenamed{"prjfs.preRenamed_us"};
  DurationStat fileHandleClosedFileDeleted{
      "prjfs.fileHandleClosedFileDeleted_us"};
  DurationStat preSetHardlink{"prjfs.preSetHardlink_us"};

  DurationStat openDir{"prjfs.opendir_us"};
  DurationStat readDir{"prjfs.readdir_us"};
  DurationStat lookup{"prjfs.lookup_us"};
  DurationStat access{"prjfs.access_us"};
  DurationStat read{"prjfs.read_us"};
#endif

  using StatPtr = DurationStat ChannelThreadStats::*;
};

/**
 * @see ObjectStore
 */
class ObjectStoreThreadStats : public EdenThreadStatsBase {
 public:
  Stat getBlobFromLocalStore{createStat("object_store.get_blob.local_store")};
  Stat getBlobFromBackingStore{
      createStat("object_store.get_blob.backing_store")};

  Stat getBlobMetadataFromMemory{
      createStat("object_store.get_blob_metadata.memory")};
  Stat getBlobMetadataFromLocalStore{
      createStat("object_store.get_blob_metadata.local_store")};
  Stat getBlobMetadataFromBackingStore{
      createStat("object_store.get_blob_metadata.backing_store")};
  Stat getLocalBlobMetadataFromBackingStore{
      createStat("object_store.get_blob_metadata.backing_store_cache")};

  Stat getBlobSizeFromLocalStore{
      createStat("object_store.get_blob_size.local_store")};
  Stat getBlobSizeFromBackingStore{
      createStat("object_store.get_blob_size.backing_store")};
};

/**
 * @see HgBackingStore
 */
class HgBackingStoreThreadStats : public EdenThreadStatsBase {
 public:
  Stat hgBackingStoreGetBlob{createStat("store.hg.get_blob")};
  Stat hgBackingStoreImportBlob{createStat("store.hg.import_blob")};
  Stat hgBackingStoreGetTree{createStat("store.hg.get_tree")};
  Stat hgBackingStoreImportTree{createStat("store.hg.import_tree")};
};

/**
 * @see HgImporter
 * @see HgBackingStore
 */
class HgImporterThreadStats : public EdenThreadStatsBase {
 public:
  Stat catFile{createStat("hg_importer.cat_file")};
  Stat fetchTree{createStat("hg_importer.fetch_tree")};
  Stat manifest{createStat("hg_importer.manifest")};
  Stat manifestNodeForCommit{
      createStat("hg_importer.manifest_node_for_commit")};
  Stat prefetchFiles{createStat("hg_importer.prefetch_files")};
};

class JournalThreadStats : public EdenThreadStatsBase {
 public:
  Stat truncatedReads{createStat("journal.truncated_reads")};
  Stat filesAccumulated{createStat("journal.files_accumulated")};
};

class ThriftThreadStats : public EdenThreadStatsBase {
 public:
  DurationStat streamChangesSince{
      "thrift.StreamingEdenService.streamChangesSince.streaming_time_us"};

  using StatPtr = DurationStat ThriftThreadStats::*;
};

} // namespace facebook::eden
