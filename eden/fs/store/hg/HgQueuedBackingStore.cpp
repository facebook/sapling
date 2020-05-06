/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/hg/HgQueuedBackingStore.h"

#include <gflags/gflags.h>
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
#include "eden/fs/utils/EnumValue.h"

namespace facebook {
namespace eden {

DEFINE_uint64(hg_queue_batch_size, 5, "Number of requests per Hg import batch");

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

void HgQueuedBackingStore::processBlobImportRequests(
    std::vector<HgImportRequest>&& requests) {
  for (auto& request : requests) {
    auto parameter = request.getRequest<HgImportRequest::BlobImport>();
    request.setWith<HgImportRequest::BlobImport>(
        [store = backingStore_.get(), hash = parameter->hash]() {
          return store->getBlob(hash).getTry();
        });
  }
}

void HgQueuedBackingStore::processTreeImportRequests(
    std::vector<HgImportRequest>&& requests) {
  for (auto& request : requests) {
    auto parameter = request.getRequest<HgImportRequest::TreeImport>();
    request.setWith<HgImportRequest::TreeImport>(
        [store = backingStore_.get(), hash = parameter->hash]() {
          return store->getTree(hash).getTry();
        });
  }
}

void HgQueuedBackingStore::processPrefetchRequests(
    std::vector<HgImportRequest>&& requests) {
  for (auto& request : requests) {
    auto parameter = request.getRequest<HgImportRequest::Prefetch>();
    request.setWith<HgImportRequest::Prefetch>(
        [store = backingStore_.get(), hashes = parameter->hashes]() {
          return store->prefetchBlobs(hashes).getTry();
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

} // namespace eden
} // namespace facebook
