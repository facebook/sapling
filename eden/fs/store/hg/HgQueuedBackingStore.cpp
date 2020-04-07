/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/hg/HgQueuedBackingStore.h"

#include <folly/executors/thread_factory/NamedThreadFactory.h>
#include <folly/futures/Future.h>
#include <utility>
#include <variant>

#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/store/hg/HgBackingStore.h"
#include "eden/fs/store/hg/HgImportRequest.h"

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

    if (auto parameter = request->getRequest<HgImportRequest::BlobImport>()) {
      auto future = folly::makeSemiFutureWith(
          [store = backingStore_.get(), hash = std::move(parameter->hash)]() {
            return store->getBlob(hash);
          });
      request->setTry<HgImportRequest::BlobImport::Response>(
          std::move(future).getTry());
    } else if (
        auto parameter = request->getRequest<HgImportRequest::TreeImport>()) {
      auto future = folly::makeSemiFutureWith(
          [store = backingStore_.get(), hash = std::move(parameter->hash)]() {
            return store->getTree(hash);
          });
      request->setTry<HgImportRequest::TreeImport::Response>(
          std::move(future).getTry());
    } else if (
        auto parameter = request->getRequest<HgImportRequest::Prefetch>()) {
      auto future =
          folly::makeSemiFutureWith([store = backingStore_.get(),
                                     hashes = std::move(parameter->hashes)]() {
            return store->prefetchBlobs(std::move(hashes));
          });
      request->setTry<HgImportRequest::Prefetch::Response>(
          std::move(future).getTry());
    }
  }
}

folly::SemiFuture<std::unique_ptr<Tree>> HgQueuedBackingStore::getTree(
    const Hash& id,
    ImportPriority priority) {
  auto [request, future] = HgImportRequest::makeTreeImportRequest(id, priority);
  queue_.enqueue(std::move(request));
  return std::move(future);
}

folly::SemiFuture<std::unique_ptr<Blob>> HgQueuedBackingStore::getBlob(
    const Hash& id,
    ImportPriority priority) {
  auto [request, future] = HgImportRequest::makeBlobImportRequest(id, priority);
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
  auto [request, future] =
      HgImportRequest::makePrefetchRequest(ids, ImportPriority::kNormal());
  queue_.enqueue(std::move(request));

  return std::move(future);
}
} // namespace eden
} // namespace facebook
