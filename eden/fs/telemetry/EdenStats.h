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

  explicit EdenThreadStatsBase();

 protected:
  Stat createStat(const std::string& name);
};

class ChannelThreadStats : public EdenThreadStatsBase {
 public:
  // We track latency in units of microseconds, hence the _us suffix in the
  // stat names below.

#ifndef _WIN32
  Stat lookup{createStat("fuse.lookup_us")};
  Stat forget{createStat("fuse.forget_us")};
  Stat getattr{createStat("fuse.getattr_us")};
  Stat setattr{createStat("fuse.setattr_us")};
  Stat readlink{createStat("fuse.readlink_us")};
  Stat mknod{createStat("fuse.mknod_us")};
  Stat mkdir{createStat("fuse.mkdir_us")};
  Stat unlink{createStat("fuse.unlink_us")};
  Stat rmdir{createStat("fuse.rmdir_us")};
  Stat symlink{createStat("fuse.symlink_us")};
  Stat rename{createStat("fuse.rename_us")};
  Stat link{createStat("fuse.link_us")};
  Stat open{createStat("fuse.open_us")};
  Stat read{createStat("fuse.read_us")};
  Stat write{createStat("fuse.write_us")};
  Stat flush{createStat("fuse.flush_us")};
  Stat release{createStat("fuse.release_us")};
  Stat fsync{createStat("fuse.fsync_us")};
  Stat opendir{createStat("fuse.opendir_us")};
  Stat readdir{createStat("fuse.readdir_us")};
  Stat releasedir{createStat("fuse.releasedir_us")};
  Stat fsyncdir{createStat("fuse.fsyncdir_us")};
  Stat statfs{createStat("fuse.statfs_us")};
  Stat setxattr{createStat("fuse.setxattr_us")};
  Stat getxattr{createStat("fuse.getxattr_us")};
  Stat listxattr{createStat("fuse.listxattr_us")};
  Stat removexattr{createStat("fuse.removexattr_us")};
  Stat access{createStat("fuse.access_us")};
  Stat create{createStat("fuse.create_us")};
  Stat bmap{createStat("fuse.bmap_us")};
  Stat ioctl{createStat("fuse.ioctl_us")};
  Stat poll{createStat("fuse.poll_us")};
  Stat forgetmulti{createStat("fuse.forgetmulti_us")};
  Stat fallocate{createStat("fuse.fallocate_us")};

  Stat nfsNull{createStat("nfs.null_us")};
  Stat nfsGetattr{createStat("nfs.getattr_us")};
  Stat nfsSetattr{createStat("nfs.setattr_us")};
  Stat nfsLookup{createStat("nfs.lookup_us")};
  Stat nfsAccess{createStat("nfs.access_us")};
  Stat nfsReadlink{createStat("nfs.readlink_us")};
  Stat nfsRead{createStat("nfs.read_us")};
  Stat nfsWrite{createStat("nfs.write_us")};
  Stat nfsCreate{createStat("nfs.create_us")};
  Stat nfsMkdir{createStat("nfs.mkdir_us")};
  Stat nfsSymlink{createStat("nfs.symlink_us")};
  Stat nfsMknod{createStat("nfs.mknod_us")};
  Stat nfsRemove{createStat("nfs.remove_us")};
  Stat nfsRmdir{createStat("nfs.rmdir_us")};
  Stat nfsRename{createStat("nfs.rename_us")};
  Stat nfsLink{createStat("nfs.link_us")};
  Stat nfsReaddir{createStat("nfs.readdir_us")};
  Stat nfsReaddirplus{createStat("nfs.readdirplus_us")};
  Stat nfsFsstat{createStat("nfs.fsstat_us")};
  Stat nfsFsinfo{createStat("nfs.fsinfo_us")};
  Stat nfsPathconf{createStat("nfs.pathconf_us")};
  Stat nfsCommit{createStat("nfs.commit_us")};
#else
  Stat outOfOrderCreate{createStat("prjfs.out_of_order_create")};

  Stat newFileCreated{createStat("prjfs.newFileCreated_us")};
  Stat fileOverwritten{createStat("prjfs.fileOverwritten_us")};
  Stat fileHandleClosedFileModified{
      createStat("prjfs.fileHandleClosedFileModified_us")};
  Stat fileRenamed{createStat("prjfs.fileRenamed_us")};
  Stat preDelete{createStat("prjfs.preDelete_us")};
  Stat preRenamed{createStat("prjfs.preRenamed_us")};
  Stat fileHandleClosedFileDeleted{
      createStat("prjfs.fileHandleClosedFileDeleted_us")};
  Stat preSetHardlink{createStat("prjfs.preSetHardlink_us")};

  Stat openDir{createStat("prjfs.opendir_us")};
  Stat readDir{createStat("prjfs.readdir_us")};
  Stat lookup{createStat("prjfs.lookup_us")};
  Stat access{createStat("prjfs.access_us")};
  Stat read{createStat("prjfs.read_us")};
#endif

  // Since we can potentially finish a request in a different thread from the
  // one used to initiate it, we use StatPtr as a helper for referencing the
  // pointer-to-member that we want to update at the end of the request.
  using StatPtr = Stat ChannelThreadStats::*;

  /**
   * Record a the latency for an operation.
   *
   * item is the pointer-to-member for one of the stats defined above.
   * elapsed is the duration of the operation, measured in microseconds.
   */
  void recordLatency(StatPtr item, std::chrono::microseconds elapsed);
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
  Stat streamChangesSince{createStat(
      "thrift.StreamingEdenService.streamChangesSince.streaming_time_us")};

  using StatPtr = Stat ThriftThreadStats::*;
  void recordLatency(StatPtr item, std::chrono::microseconds elapsed);
};

} // namespace facebook::eden
