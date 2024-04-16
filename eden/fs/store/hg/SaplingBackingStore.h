/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>
#include <folly/Synchronized.h>
#include <gtest/gtest_prod.h>
#include <sys/types.h>
#include <atomic>
#include <memory>
#include <vector>

#include "eden/common/telemetry/RequestMetricsScope.h"
#include "eden/common/telemetry/TraceBus.h"
#include "eden/common/utils/RefPtr.h"
#include "eden/fs/eden-config.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/store/BackingStore.h"
#include "eden/fs/store/ImportPriority.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/store/hg/SaplingBackingStoreOptions.h"
#include "eden/fs/store/hg/SaplingImportRequestQueue.h"
#include "eden/fs/telemetry/ActivityBuffer.h"
#include "eden/scm/lib/backingstore/include/SaplingNativeBackingStore.h"

namespace facebook::eden {

class BackingStoreLogger;
class ReloadableConfig;
class LocalStore;
class UnboundedQueueExecutor;
class EdenStats;
class SaplingImportRequest;
class StructuredLogger;
class FaultInjector;
template <typename T>
class RefPtr;
class ObjectFetchContext;
using ObjectFetchContextPtr = RefPtr<ObjectFetchContext>;

struct HgImportTraceEvent : TraceEventBase {
  enum EventType : uint8_t {
    QUEUE,
    START,
    FINISH,
  };

  enum ResourceType : uint8_t {
    BLOB,
    TREE,
    BLOBMETA,
  };

  static HgImportTraceEvent queue(
      uint64_t unique,
      ResourceType resourceType,
      const HgProxyHash& proxyHash,
      ImportPriority::Class priority,
      ObjectFetchContext::Cause cause,
      OptionalProcessId pid) {
    return HgImportTraceEvent{
        unique, QUEUE, resourceType, proxyHash, priority, cause, pid};
  }

  static HgImportTraceEvent start(
      uint64_t unique,
      ResourceType resourceType,
      const HgProxyHash& proxyHash,
      ImportPriority::Class priority,
      ObjectFetchContext::Cause cause,
      OptionalProcessId pid) {
    return HgImportTraceEvent{
        unique, START, resourceType, proxyHash, priority, cause, pid};
  }

  static HgImportTraceEvent finish(
      uint64_t unique,
      ResourceType resourceType,
      const HgProxyHash& proxyHash,
      ImportPriority::Class priority,
      ObjectFetchContext::Cause cause,
      OptionalProcessId pid) {
    return HgImportTraceEvent{
        unique, FINISH, resourceType, proxyHash, priority, cause, pid};
  }

  HgImportTraceEvent(
      uint64_t unique,
      EventType eventType,
      ResourceType resourceType,
      const HgProxyHash& proxyHash,
      ImportPriority::Class priority,
      ObjectFetchContext::Cause cause,
      OptionalProcessId pid);

  // Simple accessor that hides the internal memory representation of paths.
  std::string getPath() const {
    return path.get();
  }

  // Unique per request, but is consistent across the three stages of an import:
  // queue, start, and finish. Used to correlate events to a request.
  uint64_t unique;
  // Always null-terminated, and saves space in the trace event structure.
  // TODO: Replace with a single pointer to a reference-counted string to save 8
  // bytes in this struct.
  std::shared_ptr<char[]> path;
  // The HG manifest node ID.
  Hash20 manifestNodeId;
  EventType eventType;
  ResourceType resourceType;
  ImportPriority::Class importPriority;
  ObjectFetchContext::Cause importCause;
  OptionalProcessId pid;
};

/**
 * A Sapling backing store implementation that will put incoming blob/tree
 * import requests into a job queue, then a pool of workers will work on
 * fulfilling these requests via different methods (reading from hgcache,
 * Mononoke, debugimporthelper, etc.).
 */
class SaplingBackingStore final : public BackingStore {
 public:
  using ImportRequestsList = std::vector<std::shared_ptr<SaplingImportRequest>>;
  using SaplingNativeOptions = sapling::SaplingNativeBackingStoreOptions;
  using ImportRequestsMap = std::
      map<sapling::NodeId, std::pair<ImportRequestsList, RequestMetricsScope>>;

  SaplingBackingStore(
      AbsolutePathPiece repository,
      std::shared_ptr<LocalStore> localStore,
      EdenStatsPtr stats,
      UnboundedQueueExecutor* serverThreadPool,
      std::shared_ptr<ReloadableConfig> config,
      std::unique_ptr<SaplingBackingStoreOptions> runtimeOptions,
      std::shared_ptr<StructuredLogger> structuredLogger,
      std::unique_ptr<BackingStoreLogger> logger,
      FaultInjector* FOLLY_NONNULL faultInjector);

  /**
   * Create an SaplingBackingStore suitable for use in unit tests. It uses an
   * inline executor to process loaded objects rather than the thread pools used
   * in production Eden.
   */
  SaplingBackingStore(
      AbsolutePathPiece repository,
      std::shared_ptr<LocalStore> localStore,
      EdenStatsPtr stats,
      std::shared_ptr<ReloadableConfig> config,
      std::unique_ptr<SaplingBackingStoreOptions> runtimeOptions,
      std::shared_ptr<StructuredLogger> structuredLogger,
      std::unique_ptr<BackingStoreLogger> logger,
      FaultInjector* FOLLY_NONNULL faultInjector);

  ~SaplingBackingStore() override;

  /**
   * Objects that can be imported from Hg
   */
  enum SaplingImportObject {
    BLOB,
    TREE,
    BLOBMETA,
    BATCHED_BLOB,
    BATCHED_TREE,
    BATCHED_BLOBMETA,
    PREFETCH
  };
  constexpr static std::array<SaplingImportObject, 7> saplingImportObjects{
      SaplingImportObject::BLOB,
      SaplingImportObject::TREE,
      SaplingImportObject::BLOBMETA,
      SaplingImportObject::BATCHED_BLOB,
      SaplingImportObject::BATCHED_TREE,
      SaplingImportObject::BATCHED_BLOBMETA,
      SaplingImportObject::PREFETCH};

  static folly::StringPiece stringOfSaplingImportObject(
      SaplingImportObject object);

  ActivityBuffer<HgImportTraceEvent>& getActivityBuffer() {
    return activityBuffer_;
  }

  TraceBus<HgImportTraceEvent>& getTraceBus() const {
    return *traceBus_;
  }

  /**
   * Flush any pending writes to disk.
   *
   * As a side effect, this also reloads the current state of Mercurial's
   * cache, picking up any writes done by Mercurial.
   */
  void flush() {
    store_.flush();
  }

  ObjectComparison compareObjectsById(const ObjectId& one, const ObjectId& two)
      override;

  RootId parseRootId(folly::StringPiece rootId) override;
  std::string renderRootId(const RootId& rootId) override;
  ObjectId parseObjectId(folly::StringPiece objectId) override {
    return staticParseObjectId(objectId);
  }
  std::string renderObjectId(const ObjectId& objectId) override {
    return staticRenderObjectId(objectId);
  }

  static ObjectId staticParseObjectId(folly::StringPiece objectId);
  static std::string staticRenderObjectId(const ObjectId& objectId);

  std::optional<Hash20> getManifestNode(const ObjectId& commitId);

  /**
   * calculates `metric` for `object` imports that are `stage`.
   *    ex. SaplingBackingStore::getImportMetrics(
   *          RequestMetricsScope::HgImportStage::PENDING,
   *          SaplingBackingStore::SaplingImportObject::BLOB,
   *          RequestMetricsScope::Metric::COUNT,
   *        )
   *    calculates the number of blob imports that are pending
   */
  size_t getImportMetric(
      RequestMetricsScope::RequestStage stage,
      SaplingImportObject object,
      RequestMetricsScope::RequestMetric metric) const;

  void startRecordingFetch() override;
  std::unordered_set<std::string> stopRecordingFetch() override;

  ImmediateFuture<folly::Unit> importManifestForRoot(
      const RootId& rootId,
      const Hash20& manifestId,
      const ObjectFetchContextPtr& context) override;

  void periodicManagementTask() override;

  std::optional<folly::StringPiece> getRepoName() override {
    return store_.getRepoName();
  }

  LocalStoreCachingPolicy getLocalStoreCachingPolicy() const override {
    return localStoreCachingPolicy_;
  }

  int64_t dropAllPendingRequestsFromQueue() override;

 private:
  FRIEND_TEST(SaplingBackingStoreNoFaultInjectorTest, getTree);
  FRIEND_TEST(SaplingBackingStoreWithFaultInjectorTest, getTree);
  FRIEND_TEST(SaplingBackingStoreNoFaultInjectorTest, getBlob);
  FRIEND_TEST(SaplingBackingStoreWithFaultInjectorTest, getBlob);
  FRIEND_TEST(SaplingBackingStoreWithFaultInjectorTest, getTreeBatch);
  FRIEND_TEST(
      SaplingBackingStoreWithFaultInjectorIgnoreConfigTest,
      getTreeBatch);
  friend class EdenServiceHandler;

  // Forbidden copy constructor and assignment operator
  SaplingBackingStore(const SaplingBackingStore&) = delete;
  SaplingBackingStore& operator=(const SaplingBackingStore&) = delete;

  /**
   * Import the manifest for the specified revision using mercurial
   * treemanifest data.
   */
  folly::Future<TreePtr> importTreeManifest(
      const ObjectId& commitId,
      const ObjectFetchContextPtr& context);

  folly::Future<TreePtr> importTreeManifestImpl(
      Hash20 manifestNode,
      const ObjectFetchContextPtr& context);

  ImmediateFuture<GetRootTreeResult> getRootTree(
      const RootId& rootId,
      const ObjectFetchContextPtr& context) override;
  ImmediateFuture<std::shared_ptr<TreeEntry>> getTreeEntryForObjectId(
      const ObjectId& /* objectId */,
      TreeEntryType /* treeEntryType */,
      const ObjectFetchContextPtr& /* context */) override {
    throw std::domain_error("unimplemented");
  }

  void getTreeBatch(
      const ImportRequestsList& requests,
      sapling::FetchMode fetch_mode);

  folly::SemiFuture<GetTreeResult> getTree(
      const ObjectId& id,
      const ObjectFetchContextPtr& context) override;

  folly::Try<TreePtr> getTreeFromBackingStore(
      const RelativePath& path,
      const Hash20& manifestId,
      const ObjectId& edenTreeId,
      const ObjectFetchContextPtr& context);

  folly::Future<TreePtr> retryGetTree(
      const Hash20& manifestNode,
      const ObjectId& edenTreeID,
      RelativePathPiece path);

  folly::Future<TreePtr> retryGetTreeImpl(
      Hash20 manifestNode,
      ObjectId edenTreeID,
      RelativePath path,
      std::shared_ptr<LocalStore::WriteBatch> writeBatch);

  /**
   * Imports the tree identified by the given hash from the hg cache.
   * Returns nullptr if not found.
   */
  TreePtr getTreeLocal(
      const ObjectId& edenTreeId,
      const HgProxyHash& proxyHash);

  /**
   * Imports the tree identified by the given hash from the remote store.
   * Returns nullptr if not found.
   */
  folly::Try<TreePtr> getTreeRemote(
      const RelativePath& path,
      const Hash20& manifestId,
      const ObjectId& edenTreeId,
      const ObjectFetchContextPtr& context);

  /**
   * Fetch a tree from Mercurial.
   *
   * For latency sensitive context, the caller is responsible for checking if
   * the tree is present locally, as this function will always push the request
   * at the end of the queue.
   */
  ImmediateFuture<GetTreeResult> getTreeEnqueue(
      const ObjectId& id,
      const HgProxyHash& proxyHash,
      const ObjectFetchContextPtr& context);

  folly::SemiFuture<GetBlobResult> getBlob(
      const ObjectId& id,
      const ObjectFetchContextPtr& context) override;

  // Get blob step functions
  folly::SemiFuture<BlobPtr> retryGetBlob(HgProxyHash hgInfo);

  /**
   * Import multiple blobs at once. The vector parameters have to be the same
   * length. Promises passed in will be resolved if a blob is successfully
   * imported. Otherwise the promise will be left untouched.
   */
  void getBlobBatch(
      const ImportRequestsList& requests,
      sapling::FetchMode fetchMode);

  /**
   * Fetch a blob from Mercurial.
   *
   * For latency sensitive context, the caller is responsible for checking if
   * the blob is present locally, as this function will always push the request
   * at the end of the queue.
   */
  ImmediateFuture<GetBlobResult> getBlobImpl(
      const ObjectId& id,
      const HgProxyHash& proxyHash,
      const ObjectFetchContextPtr& context);

  /**
   * Imports the blob identified by the given hash from the backing store.
   * If localOnly is set to true, only fetch the blob from local (memory or
   * disk) store.
   *
   * Returns nullptr if not found.
   */
  folly::Try<BlobPtr> getBlobFromBackingStore(
      const HgProxyHash& hgInfo,
      sapling::FetchMode fetchMode);

  /**
   * Imports the blob identified by the given hash from the hg cache.
   * Returns nullptr if not found.
   */
  folly::Try<BlobPtr> getBlobLocal(const HgProxyHash& hgInfo) {
    return getBlobFromBackingStore(hgInfo, sapling::FetchMode::LocalOnly);
  }

  /**
   * Imports the blob identified by the given hash from the remote store.
   * Returns nullptr if not found.
   */
  folly::Try<BlobPtr> getBlobRemote(const HgProxyHash& hgInfo) {
    return getBlobFromBackingStore(hgInfo, sapling::FetchMode::RemoteOnly);
  }

  folly::SemiFuture<GetBlobMetaResult> getBlobMetadata(
      const ObjectId& id,
      const ObjectFetchContextPtr& context) override;

  /**
   * Fetch the blob metadata from Mercurial.
   *
   * For latency sensitive context, the caller is responsible for checking if
   * the blob metadata is present locally, as this function will always push
   * the request at the end of the queue.
   */
  ImmediateFuture<GetBlobMetaResult> getBlobMetadataImpl(
      const ObjectId& id,
      const HgProxyHash& proxyHash,
      const ObjectFetchContextPtr& context);

  /**
   * Fetch multiple aux data at once.
   *
   * This function returns when all the aux data have been fetched.
   */
  void getBlobMetadataBatch(
      const ImportRequestsList& requests,
      sapling::FetchMode fetch_mode);

  /**
   * Reads blob metadata from hg cache.
   */
  folly::Try<BlobMetadataPtr> getLocalBlobMetadata(const HgProxyHash& id);

  FOLLY_NODISCARD virtual folly::SemiFuture<folly::Unit> prefetchBlobs(
      ObjectIdRange ids,
      const ObjectFetchContextPtr& context) override;

  void processBlobImportRequests(
      std::vector<std::shared_ptr<SaplingImportRequest>>&& requests);
  void processTreeImportRequests(
      std::vector<std::shared_ptr<SaplingImportRequest>>&& requests);
  void processBlobMetaImportRequests(
      std::vector<std::shared_ptr<SaplingImportRequest>>&& requests);

  /**
   * The worker runloop function.
   */
  void processRequest();

  void logMissingProxyHash();

  /**
   * Logs a backing store fetch to scuba if the path being fetched is in the
   * configured paths to log. The path is derived from the proxy hash.
   */
  void logBackingStoreFetch(
      const ObjectFetchContext& context,
      folly::Range<HgProxyHash*> hashes,
      ObjectFetchContext::ObjectType type);

  /**
   * gets the watches timing `object` imports that are `stage`
   *    ex. SaplingBackingStore::getImportWatches(
   *          RequestMetricsScope::HgImportStage::PENDING,
   *          SaplingBackingStore::SaplingImportObject::BLOB,
   *        )
   *    gets the watches timing blob imports that are pending
   */
  RequestMetricsScope::LockedRequestWatchList& getImportWatches(
      RequestMetricsScope::RequestStage stage,
      SaplingImportObject object) const;

  /**
   * Gets the watches timing pending `object` imports
   *   ex. SaplingBackingStore::getPendingImportWatches(
   *          SaplingBackingStore::SaplingImportObject::BLOB,
   *        )
   *    gets the watches timing pending blob imports
   */
  RequestMetricsScope::LockedRequestWatchList& getPendingImportWatches(
      SaplingImportObject object) const;

  /**
   * Gets the watches timing live `object` imports
   *   ex. SaplingBackingStore::getLiveImportWatches(
   *          SaplingBackingStore::SaplingImportObject::BLOB,
   *        )
   *    gets the watches timing live blob imports
   */
  RequestMetricsScope::LockedRequestWatchList& getLiveImportWatches(
      SaplingImportObject object) const;

  template <typename T>
  std::pair<ImportRequestsMap, std::vector<sapling::SaplingRequest>>
  prepareRequests(
      const ImportRequestsList& importRequests,
      const std::string& requestType);

  /**
   * isRecordingFetch_ indicates if SaplingBackingStore is recording paths
   * for fetched files. Initially we don't record paths. When
   * startRecordingFetch() is called, isRecordingFetch_ is set to true and
   * recordFetch() will record the input path. When stopRecordingFetch() is
   * called, isRecordingFetch_ is set to false and recordFetch() no longer
   * records the input path.
   */
  std::atomic<bool> isRecordingFetch_{false};
  folly::Synchronized<std::unordered_set<std::string>> fetchedFilePaths_;

  std::shared_ptr<LocalStore> localStore_;
  EdenStatsPtr stats_;

  // A set of threads processing Sapling retry requests.
  std::unique_ptr<folly::Executor> retryThreadPool_;

  /**
   * Reference to the eden config, may be a null pointer in unit tests.
   */
  std::shared_ptr<ReloadableConfig> config_;

  // The main server thread pool; we push the Futures back into
  // this pool to run their completion code to avoid clogging
  // the importer pool. Queuing in this pool can never block (which would risk
  // deadlock) or throw an exception when full (which would incorrectly fail the
  // load).
  folly::Executor* serverThreadPool_;

  /**
   * The import request queue. This queue is unbounded. This queue
   * implementation will ensure enqueue operation never blocks.
   */
  SaplingImportRequestQueue queue_;

  /**
   * The worker thread pool. These threads will be running `processRequest`
   * forever to process incoming import requests
   */
  std::vector<std::thread> threads_;

  std::shared_ptr<StructuredLogger> structuredLogger_;

  /**
   * Logger for backing store imports
   */
  std::unique_ptr<BackingStoreLogger> logger_;

  FaultInjector& faultInjector_;

  LocalStoreCachingPolicy localStoreCachingPolicy_;

  // The last time we logged a missing proxy hash so the minimum interval is
  // limited to EdenConfig::missingHgProxyHashLogInterval.
  folly::Synchronized<std::chrono::steady_clock::time_point>
      lastMissingProxyHashLog_;

  // Track metrics for queued imports
  mutable RequestMetricsScope::LockedRequestWatchList pendingImportBlobWatches_;
  mutable RequestMetricsScope::LockedRequestWatchList
      pendingImportBlobMetaWatches_;
  mutable RequestMetricsScope::LockedRequestWatchList pendingImportTreeWatches_;
  mutable RequestMetricsScope::LockedRequestWatchList
      pendingImportPrefetchWatches_;

  // Track metrics for imports currently fetching data from hg
  mutable RequestMetricsScope::LockedRequestWatchList liveImportBlobWatches_;
  mutable RequestMetricsScope::LockedRequestWatchList liveImportTreeWatches_;
  mutable RequestMetricsScope::LockedRequestWatchList
      liveImportBlobMetaWatches_;
  mutable RequestMetricsScope::LockedRequestWatchList
      liveImportPrefetchWatches_;

  // Track metrics for the number of live batches
  mutable RequestMetricsScope::LockedRequestWatchList liveBatchedBlobWatches_;
  mutable RequestMetricsScope::LockedRequestWatchList liveBatchedTreeWatches_;
  mutable RequestMetricsScope::LockedRequestWatchList
      liveBatchedBlobMetaWatches_;

  std::unique_ptr<SaplingBackingStoreOptions> runtimeOptions_;

  ActivityBuffer<HgImportTraceEvent> activityBuffer_;

  // The traceBus_ and hgTraceHandle_ should be last so any internal subscribers
  // can capture [this].
  std::shared_ptr<TraceBus<HgImportTraceEvent>> traceBus_;

  // Handle for TraceBus subscription.
  TraceSubscriptionHandle<HgImportTraceEvent> hgTraceHandle_;

  sapling::SaplingNativeBackingStore store_;
};

} // namespace facebook::eden
