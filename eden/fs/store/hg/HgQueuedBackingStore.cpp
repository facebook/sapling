/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/hg/HgQueuedBackingStore.h"

#include <utility>
#include <variant>

#include <folly/String.h>
#include <folly/executors/thread_factory/NamedThreadFactory.h>
#include <folly/futures/Future.h>

#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/store/hg/HgBackingStore.h"
#include "eden/fs/store/hg/HgImportRequest.h"
#include "eden/fs/telemetry/RequestMetricsScope.h"
#include "eden/fs/utils/Bug.h"

namespace facebook {
namespace eden {

HgQueuedBackingStore::HgQueuedBackingStore(
    std::unique_ptr<HgBackingStore> backingStore,
    uint8_t numberThreads)
    : backingStore_(std::move(backingStore)) {
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

void HgQueuedBackingStore::processRequest() {
  for (;;) {
    auto request = queue_.dequeue();

    if (!request) {
      break;
    }
    auto requestTracker = request->getOwnershipOfImportTracker();

    if (auto parameter = request->getRequest<HgImportRequest::BlobImport>()) {
      auto future =
          folly::makeSemiFutureWith([store = backingStore_.get(),
                                     hash = std::move(parameter->hash)]() {
            return store->getBlob(hash);
          })
              .defer([tracker = std::move(requestTracker)](auto&& result) {
                return folly::makeSemiFuture(std::move(result));
              });
      request->setTry<HgImportRequest::BlobImport::Response>(
          std::move(future).getTry());
    } else if (
        auto parameter = request->getRequest<HgImportRequest::TreeImport>()) {
      auto future =
          folly::makeSemiFutureWith([store = backingStore_.get(),
                                     hash = std::move(parameter->hash)]() {
            return store->getTree(hash);
          })
              .defer([tracker = std::move(requestTracker)](auto&& result) {
                return folly::makeSemiFuture(std::move(result));
              });
      request->setTry<HgImportRequest::TreeImport::Response>(
          std::move(future).getTry());
    } else if (
        auto parameter = request->getRequest<HgImportRequest::Prefetch>()) {
      auto future =
          folly::makeSemiFutureWith([store = backingStore_.get(),
                                     hashes = std::move(parameter->hashes)]() {
            return store->prefetchBlobs(std::move(hashes));
          })
              .defer([tracker = std::move(requestTracker)](auto&& result) {
                return folly::makeSemiFuture(std::move(result));
              });
      request->setTry<HgImportRequest::Prefetch::Response>(
          std::move(future).getTry());
    }
  }
}

folly::SemiFuture<std::unique_ptr<Tree>> HgQueuedBackingStore::getTree(
    const Hash& id,
    ImportPriority priority) {
  auto importTracker =
      std::make_unique<RequestMetricsScope>(&pendingImportTreeWatches_);
  auto [request, future] = HgImportRequest::makeTreeImportRequest(
      id, priority, std::move(importTracker));
  queue_.enqueue(std::move(request));
  return std::move(future);
}

folly::SemiFuture<std::unique_ptr<Blob>> HgQueuedBackingStore::getBlob(
    const Hash& id,
    ImportPriority priority) {
  auto importTracker =
      std::make_unique<RequestMetricsScope>(&pendingImportBlobWatches_);
  auto [request, future] = HgImportRequest::makeBlobImportRequest(
      id, priority, std::move(importTracker));
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
    const std::vector<Hash>& ids) {
  auto importTracker =
      std::make_unique<RequestMetricsScope>(&pendingImportPrefetchWatches_);
  auto [request, future] = HgImportRequest::makePrefetchRequest(
      ids, ImportPriority::kNormal(), std::move(importTracker));
  queue_.enqueue(std::move(request));

  return std::move(future);
}

folly::StringPiece HgQueuedBackingStore::stringOfHgImportStage(
    HgImportStage stage) {
  switch (stage) {
    case HgImportStage::PENDING:
      return "pending_import";
    case HgImportStage::LIVE:
      return "live_import";
  }
  EDEN_BUG() << "unknown hg import stage " << static_cast<int>(stage);
}

size_t HgQueuedBackingStore::getImportMetric(
    HgImportStage stage,
    HgBackingStore::HgImportObject object,
    RequestMetricsScope::RequestMetric metric) const {
  return RequestMetricsScope::getMetricFromWatches(
      metric, getImportWatches(stage, object));
}

RequestMetricsScope::LockedRequestWatchList&
HgQueuedBackingStore::getImportWatches(
    HgImportStage stage,
    HgBackingStore::HgImportObject object) const {
  switch (stage) {
    case HgImportStage::PENDING:
      return getPendingImportWatches(object);
    case HgImportStage::LIVE:
      return backingStore_->getLiveImportWatches(object);
  }
  EDEN_BUG() << "unknown hg import stage " << static_cast<int>(stage);
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

} // namespace eden
} // namespace facebook
