/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include <fb303/ThreadLocalStats.h>
#include <folly/ThreadLocal.h>
#include <memory>

#include "eden/fs/eden-config.h"

namespace facebook {
namespace eden {

class FuseThreadStats;
class ObjectStoreThreadStats;
class HgBackingStoreThreadStats;
class HgImporterThreadStats;
class JournalThreadStats;

class EdenStats {
 public:
  /**
   * This function can be called on any thread.
   *
   * The returned object can be used only on the current thread.
   */
  FuseThreadStats& getFuseStatsForCurrentThread();

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
   */
  void aggregate();

 private:
  class ThreadLocalTag {};

  folly::ThreadLocal<FuseThreadStats, ThreadLocalTag, void>
      threadLocalFuseStats_;
  folly::ThreadLocal<ObjectStoreThreadStats, ThreadLocalTag, void>
      threadLocalObjectStoreStats_;
  folly::ThreadLocal<HgBackingStoreThreadStats, ThreadLocalTag, void>
      threadLocalHgBackingStoreStats_;
  folly::ThreadLocal<HgImporterThreadStats, ThreadLocalTag, void>
      threadLocalHgImporterStats_;
  folly::ThreadLocal<JournalThreadStats, ThreadLocalTag, void>
      threadLocalJournalStats_;
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
class EdenThreadStatsBase
    : public fb303::ThreadLocalStatsT<fb303::TLStatsThreadSafe> {
 public:
  using Histogram = TLHistogram;
  using Timeseries = TLTimeseries;

  explicit EdenThreadStatsBase();

 protected:
  Histogram createHistogram(const std::string& name);
  Timeseries createTimeseries(const std::string& name);
};

class FuseThreadStats : public EdenThreadStatsBase {
 public:
  // We track latency in units of microseconds, hence the _us suffix
  // in the histogram names below.

  Histogram lookup{createHistogram("fuse.lookup_us")};
  Histogram forget{createHistogram("fuse.forget_us")};
  Histogram getattr{createHistogram("fuse.getattr_us")};
  Histogram setattr{createHistogram("fuse.setattr_us")};
  Histogram readlink{createHistogram("fuse.readlink_us")};
  Histogram mknod{createHistogram("fuse.mknod_us")};
  Histogram mkdir{createHistogram("fuse.mkdir_us")};
  Histogram unlink{createHistogram("fuse.unlink_us")};
  Histogram rmdir{createHistogram("fuse.rmdir_us")};
  Histogram symlink{createHistogram("fuse.symlink_us")};
  Histogram rename{createHistogram("fuse.rename_us")};
  Histogram link{createHistogram("fuse.link_us")};
  Histogram open{createHistogram("fuse.open_us")};
  Histogram read{createHistogram("fuse.read_us")};
  Histogram write{createHistogram("fuse.write_us")};
  Histogram flush{createHistogram("fuse.flush_us")};
  Histogram release{createHistogram("fuse.release_us")};
  Histogram fsync{createHistogram("fuse.fsync_us")};
  Histogram opendir{createHistogram("fuse.opendir_us")};
  Histogram readdir{createHistogram("fuse.readdir_us")};
  Histogram releasedir{createHistogram("fuse.releasedir_us")};
  Histogram fsyncdir{createHistogram("fuse.fsyncdir_us")};
  Histogram statfs{createHistogram("fuse.statfs_us")};
  Histogram setxattr{createHistogram("fuse.setxattr_us")};
  Histogram getxattr{createHistogram("fuse.getxattr_us")};
  Histogram listxattr{createHistogram("fuse.listxattr_us")};
  Histogram removexattr{createHistogram("fuse.removexattr_us")};
  Histogram access{createHistogram("fuse.access_us")};
  Histogram create{createHistogram("fuse.create_us")};
  Histogram bmap{createHistogram("fuse.bmap_us")};
  Histogram ioctl{createHistogram("fuse.ioctl_us")};
  Histogram poll{createHistogram("fuse.poll_us")};
  Histogram forgetmulti{createHistogram("fuse.forgetmulti_us")};

  // Since we can potentially finish a request in a different
  // thread from the one used to initiate it, we use HistogramPtr
  // as a helper for referencing the pointer-to-member that we
  // want to update at the end of the request.
  using HistogramPtr = Histogram FuseThreadStats::*;

  /** Record a the latency for an operation.
   * item is the pointer-to-member for one of the histograms defined
   * above.
   * elapsed is the duration of the operation, measured in microseconds.
   * now is the current steady clock value in seconds.
   * (Once we open source the common stats code we can eliminate the
   * now parameter from this method). */
  void recordLatency(
      HistogramPtr item,
      std::chrono::microseconds elapsed,
      std::chrono::seconds now);
};

/**
 * @see ObjectStore
 */
class ObjectStoreThreadStats : public EdenThreadStatsBase {
 public:
  Timeseries getBlobFromLocalStore{
      createTimeseries("object_store.get_blob.local_store")};
  Timeseries getBlobFromBackingStore{
      createTimeseries("object_store.get_blob.backing_store")};

  Timeseries getBlobMetadataFromMemory{
      createTimeseries("object_store.get_blob_metadata.memory")};
  Timeseries getBlobMetadataFromLocalStore{
      createTimeseries("object_store.get_blob_metadata.local_store")};
  Timeseries getBlobMetadataFromBackingStore{
      createTimeseries("object_store.get_blob_metadata.backing_store")};

  Timeseries getBlobSizeFromLocalStore{
      createTimeseries("object_store.get_blob_size.local_store")};
  Timeseries getBlobSizeFromBackingStore{
      createTimeseries("object_store.get_blob_size.backing_store")};
};

/**
 * @see HgBackingStore
 */
class HgBackingStoreThreadStats : public EdenThreadStatsBase {
 public:
  Histogram hgBackingStoreGetBlob{createHistogram("store.hg.get_blob")};
  Histogram hgBackingStoreGetTree{createHistogram("store.hg.get_tree")};
  Histogram mononokeBackingStoreGetTree{
      createHistogram("store.mononoke.get_tree")};
  Histogram mononokeBackingStoreGetBlob{
      createHistogram("store.mononoke.get_blob")};
};

/**
 * @see HgImporter
 * @see HgBackingStore
 */
class HgImporterThreadStats : public EdenThreadStatsBase {
 public:
  Timeseries catFile{createTimeseries("hg_importer.cat_file")};
  Timeseries fetchTree{createTimeseries("hg_importer.fetch_tree")};
  Timeseries manifest{createTimeseries("hg_importer.manifest")};
  Timeseries manifestNodeForCommit{
      createTimeseries("hg_importer.manifest_node_for_commit")};
  Timeseries prefetchFiles{createTimeseries("hg_importer.prefetch_files")};
};

class JournalThreadStats : public EdenThreadStatsBase {};

} // namespace eden
} // namespace facebook
