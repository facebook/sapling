/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/hg/HgQueuedBackingStore.h"

#include <folly/futures/Future.h>
#include <folly/logging/xlog.h>
#include <gflags/gflags.h>
#include <thread>
#include <utility>
#include <variant>

#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/store/BackingStoreLogger.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/store/hg/HgBackingStore.h"
#include "eden/fs/store/hg/HgImportRequest.h"
#include "eden/fs/store/hg/HgProxyHash.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/RequestMetricsScope.h"
#include "eden/fs/utils/Bug.h"
#include "eden/fs/utils/EnumValue.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook {
namespace eden {

DEFINE_uint64(hg_queue_batch_size, 1, "Number of requests per Hg import batch");

HgQueuedBackingStore::HgQueuedBackingStore(
    std::shared_ptr<LocalStore> localStore,
    std::shared_ptr<EdenStats> stats,
    std::unique_ptr<HgBackingStore> backingStore,
    std::shared_ptr<ReloadableConfig> config,
    std::unique_ptr<BackingStoreLogger> logger,
    uint8_t numberThreads)
    : localStore_(std::move(localStore)),
      stats_(std::move(stats)),
      config_(std::move(config)),
      backingStore_(std::move(backingStore)),
      logger_(std::move(logger)) {
  threads_.reserve(numberThreads);
  for (int i = 0; i < numberThreads; i++) {
    threads_.emplace_back(&HgQueuedBackingStore::processRequest, this);
  }
}

HgQueuedBackingStore::~HgQueuedBackingStore() {
  queue_.stop();
  for (auto& thread : threads_) {
    thread.join();
  }
}

void HgQueuedBackingStore::processBlobImportRequests(
    std::vector<HgImportRequest>&& requests) {
  std::vector<Hash> hashes;
  std::vector<folly::Promise<HgImportRequest::BlobImport::Response>*> promises;

  folly::stop_watch<std::chrono::milliseconds> watch;
  hashes.reserve(requests.size());
  promises.reserve(requests.size());

  XLOG(DBG4) << "Processing blob import batch size=" << requests.size();

  for (auto& request : requests) {
    auto& hash = request.getRequest<HgImportRequest::BlobImport>()->hash;
    auto* promise = request.getPromise<HgImportRequest::BlobImport::Response>();

    XLOGF(
        DBG4,
        "Processing blob request for {} ({:p})",
        hash.toString(),
        static_cast<void*>(promise));
    hashes.emplace_back(hash);
    promises.emplace_back(promise);
  }

  auto proxyHashesTry =
      HgProxyHash::getBatch(localStore_.get(), hashes).wait().result();

  if (proxyHashesTry.hasException()) {
    // TODO(zeyi): We should change HgProxyHash::getBatch to make it return
    // partial result instead of fail the entire batch.
    XLOG(WARN) << "Failed to get proxy hash: "
               << proxyHashesTry.exception().what();

    for (auto& request : requests) {
      request.getPromise<HgImportRequest::BlobImport::Response>()->setException(
          proxyHashesTry.exception());
    }

    return;
  }

  auto proxyHashes = proxyHashesTry.value();

  backingStore_->getDatapackStore().getBlobBatch(hashes, proxyHashes, promises);

  {
    auto request = requests.begin();
    auto proxyHash = proxyHashes.begin();
    std::vector<folly::SemiFuture<folly::Unit>> futures;
    futures.reserve(requests.size());

    XCHECK_EQ(requests.size(), proxyHashes.size());
    for (; request != requests.end(); request++, proxyHash++) {
      if (request->getPromise<HgImportRequest::BlobImport::Response>()
              ->isFulfilled()) {
        stats_->getHgBackingStoreStatsForCurrentThread()
            .hgBackingStoreGetBlob.addValue(watch.elapsed().count());
        continue;
      }

      futures.emplace_back(
          backingStore_->fetchBlobFromHgImporter(*proxyHash)
              .defer([request = std::move(*request), watch, stats = stats_](
                         auto&& result) mutable {
                auto hash =
                    request.getRequest<HgImportRequest::BlobImport>()->hash;
                XLOG(DBG4) << "Imported blob from HgImporter for " << hash;
                stats->getHgBackingStoreStatsForCurrentThread()
                    .hgBackingStoreGetBlob.addValue(watch.elapsed().count());
                request.getPromise<HgImportRequest::BlobImport::Response>()
                    ->setTry(std::forward<decltype(result)>(result));
              }));
    }

    folly::collectAll(futures).wait();
  }
}

void HgQueuedBackingStore::processTreeImportRequests(
    std::vector<HgImportRequest>&& requests) {
  for (auto& request : requests) {
    auto parameter = request.getRequest<HgImportRequest::TreeImport>();
    request.getPromise<HgImportRequest::TreeImport::Response>()->setWith(
        [store = backingStore_.get(), hash = parameter->hash]() {
          // TODO(kmancini): follow up with threading the context all the way
          // through the backing store
          return store->getTree(hash, ObjectFetchContext::getNullContext())
              .getTry();
        });
  }
}

void HgQueuedBackingStore::processPrefetchRequests(
    std::vector<HgImportRequest>&& requests) {
  for (auto& request : requests) {
    auto parameter = request.getRequest<HgImportRequest::Prefetch>();
    request.getPromise<HgImportRequest::Prefetch::Response>()->setWith(
        [store = backingStore_.get(), hashes = parameter->hashes]() {
          return store
              ->prefetchBlobs(hashes, ObjectFetchContext::getNullContext())
              .getTry();
        });
  }
}

void HgQueuedBackingStore::processRequest() {
  for (;;) {
    auto requests = queue_.dequeue(FLAGS_hg_queue_batch_size);

    if (requests.empty()) {
      break;
    }

    const auto& first = requests.at(0);

    if (first.isType<HgImportRequest::BlobImport>()) {
      processBlobImportRequests(std::move(requests));
    } else if (first.isType<HgImportRequest::TreeImport>()) {
      processTreeImportRequests(std::move(requests));
    } else if (first.isType<HgImportRequest::Prefetch>()) {
      processPrefetchRequests(std::move(requests));
    }
  }
}

folly::SemiFuture<std::unique_ptr<Tree>> HgQueuedBackingStore::getTree(
    const Hash& id,
    ObjectFetchContext& context) {
  logBackingStoreFetch(context, id);

  auto importTracker =
      std::make_unique<RequestMetricsScope>(&pendingImportTreeWatches_);
  auto [request, future] = HgImportRequest::makeTreeImportRequest(
      id, context.getPriority(), std::move(importTracker));
  queue_.enqueue(std::move(request));
  return std::move(future);
}

folly::SemiFuture<std::unique_ptr<Blob>> HgQueuedBackingStore::getBlob(
    const Hash& id,
    ObjectFetchContext& context) {
  auto proxyHash = HgProxyHash(localStore_.get(), id, "getBlob");
  auto path = proxyHash.path();
  logBackingStoreFetch(context, path);

  if (auto blob =
          backingStore_->getDatapackStore().getBlobLocal(id, proxyHash)) {
    return folly::makeSemiFuture(std::move(blob));
  }

  XLOG(DBG4) << "make blob import request for " << path << ", hash is:" << id;

  auto importTracker =
      std::make_unique<RequestMetricsScope>(&pendingImportBlobWatches_);
  auto [request, future] = HgImportRequest::makeBlobImportRequest(
      id, context.getPriority(), std::move(importTracker));
  queue_.enqueue(std::move(request));
  return std::move(future);
}

folly::SemiFuture<std::unique_ptr<Tree>> HgQueuedBackingStore::getTreeForCommit(
    const Hash& commitID) {
  return backingStore_->getTreeForCommit(commitID);
}

folly::SemiFuture<std::unique_ptr<Tree>>
HgQueuedBackingStore::getTreeForManifest(
    const Hash& commitID,
    const Hash& manifestID) {
  return backingStore_->getTreeForManifest(commitID, manifestID);
}

folly::SemiFuture<folly::Unit> HgQueuedBackingStore::prefetchBlobs(
    const std::vector<Hash>& ids,
    ObjectFetchContext& context) {
  // when useEdenNativePrefetch is true, fetch blobs one by one instead
  // of grouping them and fetching in batches.
  if (config_->getEdenConfig()->useEdenNativePrefetch.getValue()) {
    std::vector<folly::SemiFuture<std::unique_ptr<Blob>>> futures;
    futures.reserve(ids.size());
    for (auto id : ids) {
      futures.emplace_back(getBlob(id, context));
    }
    return folly::collectAll(futures).deferValue([](const auto& tries) {
      for (const auto& t : tries) {
        t.throwIfFailed();
      }
    });
  }

  for (auto& hash : ids) {
    logBackingStoreFetch(context, hash);
  }

  auto importTracker =
      std::make_unique<RequestMetricsScope>(&pendingImportPrefetchWatches_);
  auto [request, future] = HgImportRequest::makePrefetchRequest(
      ids, ImportPriority::kNormal(), std::move(importTracker));
  queue_.enqueue(std::move(request));

  return std::move(future);
}

void HgQueuedBackingStore::logBackingStoreFetch(
    ObjectFetchContext& context,
    std::variant<RelativePathPiece, Hash> identifier) {
  if (!config_) {
    return;
  }
  auto logFetchPath = config_->getEdenConfig()->logObjectFetchPath.getValue();

  if (!logFetchPath) {
    return;
  }
  std::optional<HgProxyHash> proxyHash;
  RelativePathPiece path;
  if (auto maybe_path = std::get_if<RelativePathPiece>(&identifier)) {
    path = *maybe_path;
  } else {
    auto hash = std::get<Hash>(identifier);
    try {
      proxyHash = HgProxyHash(localStore_.get(), hash, "logBackingStoreFetch");
      path = proxyHash.value().path();
    } catch (const std::domain_error&) {
      XLOG(WARN) << "Unable to get proxy hash for logging " << hash.toString();
      return;
    }
  }

  recordFetch(path.stringPiece());

  if (RelativePathPiece(logFetchPath.value())
          .isParentDirOf(RelativePathPiece(path))) {
    logger_->logImport(context, path);
  }
}

size_t HgQueuedBackingStore::getImportMetric(
    RequestMetricsScope::RequestStage stage,
    HgBackingStore::HgImportObject object,
    RequestMetricsScope::RequestMetric metric) const {
  return RequestMetricsScope::getMetricFromWatches(
      metric, getImportWatches(stage, object));
}

RequestMetricsScope::LockedRequestWatchList&
HgQueuedBackingStore::getImportWatches(
    RequestMetricsScope::RequestStage stage,
    HgBackingStore::HgImportObject object) const {
  switch (stage) {
    case RequestMetricsScope::RequestStage::PENDING:
      return getPendingImportWatches(object);
    case RequestMetricsScope::RequestStage::LIVE:
      return backingStore_->getLiveImportWatches(object);
  }
  EDEN_BUG() << "unknown hg import stage " << enumValue(stage);
}

RequestMetricsScope::LockedRequestWatchList&
HgQueuedBackingStore::getPendingImportWatches(
    HgBackingStore::HgImportObject object) const {
  switch (object) {
    case HgBackingStore::HgImportObject::BLOB:
      return pendingImportBlobWatches_;
    case HgBackingStore::HgImportObject::TREE:
      return pendingImportTreeWatches_;
    case HgBackingStore::HgImportObject::PREFETCH:
      return pendingImportPrefetchWatches_;
  }
  EDEN_BUG() << "unknown hg import object type " << static_cast<int>(object);
}

void HgQueuedBackingStore::startRecordingFetch() {
  isRecordingFetch_.store(true);
}

void HgQueuedBackingStore::recordFetch(folly::StringPiece importPath) {
  if (isRecordingFetch_.load()) {
    fetchedFilePaths_.wlock()->emplace(importPath.str());
  }
}

std::unordered_set<std::string> HgQueuedBackingStore::stopRecordingFetch() {
  isRecordingFetch_.store(false);
  std::unordered_set<std::string> paths;
  std::swap(paths, *fetchedFilePaths_.wlock());
  return paths;
}

} // namespace eden
} // namespace facebook
