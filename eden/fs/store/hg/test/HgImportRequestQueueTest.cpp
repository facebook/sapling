/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/logging/xlog.h>
#include <gtest/gtest.h>
#include <array>
#include <memory>

#include "eden/fs/model/Hash.h"
#include "eden/fs/store/ImportPriority.h"
#include "eden/fs/store/hg/HgImportRequest.h"
#include "eden/fs/store/hg/HgImportRequestQueue.h"
#include "eden/fs/telemetry/RequestMetricsScope.h"
#include "eden/fs/utils/IDGen.h"

using namespace facebook::eden;

Hash uniqueHash() {
  std::array<uint8_t, Hash::RAW_SIZE> bytes = {0};
  auto uid = generateUniqueID();
  std::memcpy(bytes.data(), &uid, sizeof(uid));
  return Hash{bytes};
}

std::pair<Hash, HgImportRequest> makeBlobImportRequest(
    ImportPriority priority,
    RequestMetricsScope::LockedRequestWatchList& pendingImportWatches) {
  auto hash = uniqueHash();
  auto importTracker =
      std::make_unique<RequestMetricsScope>(&pendingImportWatches);
  return std::make_pair(
      hash,
      HgImportRequest::makeBlobImportRequest(
          hash, priority, std::move(importTracker))
          .first);
}

std::pair<Hash, HgImportRequest> makeTreeImportRequest(
    ImportPriority priority,
    RequestMetricsScope::LockedRequestWatchList& pendingImportWatches) {
  auto hash = uniqueHash();
  auto importTracker =
      std::make_unique<RequestMetricsScope>(&pendingImportWatches);
  return std::make_pair(
      hash,
      HgImportRequest::makeTreeImportRequest(
          hash, priority, std::move(importTracker))
          .first);
}

TEST(HgImportRequestQueueTest, getRequestByPriority) {
  auto queue = HgImportRequestQueue{};
  std::vector<Hash> enqueued;
  RequestMetricsScope::LockedRequestWatchList pendingImportWatches;

  for (int i = 0; i < 10; i++) {
    auto [hash, request] = makeBlobImportRequest(
        ImportPriority(ImportPriorityKind::Normal, i), pendingImportWatches);

    queue.enqueue(std::move(request));
    enqueued.push_back(hash);
  }

  auto [smallHash, smallRequest] = makeBlobImportRequest(
      ImportPriority(ImportPriorityKind::Low, 0), pendingImportWatches);
  queue.enqueue(std::move(smallRequest));

  // the queue should give requests in the reverse order of pushing
  while (!enqueued.empty()) {
    auto expected = enqueued.back();
    enqueued.pop_back();

    EXPECT_EQ(
        expected,
        queue.dequeue(1).at(0).getRequest<HgImportRequest::BlobImport>()->hash);
  }

  EXPECT_EQ(
      smallHash,
      queue.dequeue(1).at(0).getRequest<HgImportRequest::BlobImport>()->hash);
}

TEST(HgImportRequestQueueTest, getRequestByPriorityReverse) {
  auto queue = HgImportRequestQueue{};
  std::deque<Hash> enqueued;
  RequestMetricsScope::LockedRequestWatchList pendingImportWatches;

  for (int i = 0; i < 10; i++) {
    auto [hash, request] = makeBlobImportRequest(
        ImportPriority(ImportPriorityKind::Normal, 10 - i),
        pendingImportWatches);

    queue.enqueue(std::move(request));
    enqueued.push_back(hash);
  }

  auto [largeHash, largeRequest] = makeBlobImportRequest(
      ImportPriority(ImportPriority::kHigh()), pendingImportWatches);
  queue.enqueue(std::move(largeRequest));
  EXPECT_EQ(
      largeHash,
      queue.dequeue(1).at(0).getRequest<HgImportRequest::BlobImport>()->hash);

  while (!enqueued.empty()) {
    auto expected = enqueued.front();
    enqueued.pop_front();

    auto request = queue.dequeue(1);

    EXPECT_EQ(
        expected,
        request.at(0).getRequest<HgImportRequest::BlobImport>()->hash);
  }
}

TEST(HgImportRequestQueueTest, getMultipleRequests) {
  RequestMetricsScope::LockedRequestWatchList pendingImportWatches;

  auto queue = HgImportRequestQueue{};
  std::set<Hash> enqueued_blob;

  for (int i = 0; i < 10; i++) {
    {
      auto [hash, request] = makeBlobImportRequest(
          ImportPriority(ImportPriorityKind::Normal, 0), pendingImportWatches);

      XLOG(INFO) << "enqueuing blob:" << hash;

      queue.enqueue(std::move(request));
      enqueued_blob.emplace(hash);
    }

    auto [hash, request] = makeTreeImportRequest(
        ImportPriority(ImportPriorityKind::Normal, 0), pendingImportWatches);
    XLOG(INFO) << "enqueuing tree:" << hash;
    queue.enqueue(std::move(request));
  }

  auto dequeued = queue.dequeue(20);
  EXPECT_EQ(dequeued.size(), 10);
  for (int i = 0; i < 10; i++) {
    EXPECT_TRUE(
        enqueued_blob.find(
            dequeued.at(i).getRequest<HgImportRequest::BlobImport>()->hash) !=
        enqueued_blob.end());
  }
}
