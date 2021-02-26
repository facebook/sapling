/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/Try.h>
#include <folly/logging/xlog.h>
#include <gtest/gtest.h>
#include <array>
#include <memory>

#include "eden/fs/model/Hash.h"
#include "eden/fs/store/ImportPriority.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/store/hg/HgImportRequest.h"
#include "eden/fs/store/hg/HgImportRequestQueue.h"
#include "eden/fs/store/hg/HgProxyHash.h"
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
  auto hgRevHash = uniqueHash();
  auto proxyHash = HgProxyHash{RelativePath{"some_blob"}, hgRevHash};
  auto hash = proxyHash.sha1();
  auto importTracker =
      std::make_unique<RequestMetricsScope>(&pendingImportWatches);
  return std::make_pair(
      hash,
      HgImportRequest::makeBlobImportRequest(
          hash, std::move(proxyHash), priority, std::move(importTracker))
          .first);
}

std::pair<Hash, HgImportRequest> makeBlobImportRequestWithHash(
    ImportPriority priority,
    HgProxyHash proxyHash,
    RequestMetricsScope::LockedRequestWatchList& pendingImportWatches) {
  auto hash = proxyHash.sha1();
  auto importTracker =
      std::make_unique<RequestMetricsScope>(&pendingImportWatches);
  return std::make_pair(
      hash,
      HgImportRequest::makeBlobImportRequest(
          hash, std::move(proxyHash), priority, std::move(importTracker))
          .first);
}

std::pair<Hash, HgImportRequest> makeTreeImportRequest(
    ImportPriority priority,
    RequestMetricsScope::LockedRequestWatchList& pendingImportWatches) {
  auto hgRevHash = uniqueHash();
  auto proxyHash = HgProxyHash{RelativePath{"some_tree"}, hgRevHash};
  auto hash = proxyHash.sha1();
  auto importTracker =
      std::make_unique<RequestMetricsScope>(&pendingImportWatches);
  return std::make_pair(
      hash,
      HgImportRequest::makeTreeImportRequest(
          hash, std::move(proxyHash), priority, std::move(importTracker), true)
          .first);
}

TEST(HgImportRequestQueueTest, getRequestByPriority) {
  auto queue = HgImportRequestQueue{};
  std::vector<Hash> enqueued;
  RequestMetricsScope::LockedRequestWatchList pendingImportWatches;

  for (int i = 0; i < 10; i++) {
    auto [hash, request] = makeBlobImportRequest(
        ImportPriority(ImportPriorityKind::Normal, i), pendingImportWatches);

    EXPECT_EQ(
        std::nullopt,
        queue.checkImportInProgress<Blob>(
            request.getRequest<HgImportRequest::BlobImport>()->proxyHash,
            ImportPriority(ImportPriorityKind::Normal, i)));

    queue.enqueue(std::move(request));
    enqueued.push_back(hash);
  }

  auto [smallHash, smallRequest] = makeBlobImportRequest(
      ImportPriority(ImportPriorityKind::Low, 0), pendingImportWatches);

  EXPECT_EQ(
      std::nullopt,
      queue.checkImportInProgress<Blob>(
          smallRequest.getRequest<HgImportRequest::BlobImport>()->proxyHash,
          ImportPriority(ImportPriorityKind::Low, 0)));

  queue.enqueue(std::move(smallRequest));

  // the queue should give requests in the reverse order of pushing
  while (!enqueued.empty()) {
    auto expected = enqueued.back();
    enqueued.pop_back();
    auto request = queue.dequeue(1).at(0);
    EXPECT_EQ(
        expected, request->getRequest<HgImportRequest::BlobImport>()->hash);

    folly::Try<std::unique_ptr<Blob>> blob = folly::makeTryWith([expected]() {
      return std::make_unique<Blob>(expected, folly::IOBuf{});
    });

    queue.markImportAsFinished<Blob>(
        request->getRequest<HgImportRequest::BlobImport>()->proxyHash, blob);
  }

  auto smallRequestDequeue = queue.dequeue(1).at(0);
  EXPECT_EQ(
      smallHash,
      smallRequestDequeue->getRequest<HgImportRequest::BlobImport>()->hash);

  folly::Try<std::unique_ptr<Blob>> smallBlob =
      folly::makeTryWith([smallHash = smallHash]() {
        return std::make_unique<Blob>(smallHash, folly::IOBuf{});
      });

  queue.markImportAsFinished<Blob>(
      smallRequestDequeue->getRequest<HgImportRequest::BlobImport>()->proxyHash,
      smallBlob);
}

TEST(HgImportRequestQueueTest, getRequestByPriorityReverse) {
  auto queue = HgImportRequestQueue{};
  std::deque<Hash> enqueued;
  RequestMetricsScope::LockedRequestWatchList pendingImportWatches;

  for (int i = 0; i < 10; i++) {
    auto [hash, request] = makeBlobImportRequest(
        ImportPriority(ImportPriorityKind::Normal, 10 - i),
        pendingImportWatches);

    EXPECT_EQ(
        std::nullopt,
        queue.checkImportInProgress<Blob>(
            request.getRequest<HgImportRequest::BlobImport>()->proxyHash,
            ImportPriority(ImportPriorityKind::Normal, 10 - i)));

    queue.enqueue(std::move(request));
    enqueued.push_back(hash);
  }

  auto [largeHash, largeRequest] = makeBlobImportRequest(
      ImportPriority(ImportPriority::kHigh()), pendingImportWatches);

  EXPECT_EQ(
      std::nullopt,
      queue.checkImportInProgress<Blob>(
          largeRequest.getRequest<HgImportRequest::BlobImport>()->proxyHash,
          ImportPriority(ImportPriority::kHigh())));

  queue.enqueue(std::move(largeRequest));

  auto largeHashDequeue = queue.dequeue(1).at(0);
  EXPECT_EQ(
      largeHash,
      largeHashDequeue->getRequest<HgImportRequest::BlobImport>()->hash);

  folly::Try<std::unique_ptr<Blob>> largeBlob =
      folly::makeTryWith([largeHash = largeHash]() {
        return std::make_unique<Blob>(largeHash, folly::IOBuf{});
      });
  queue.markImportAsFinished<Blob>(
      largeHashDequeue->getRequest<HgImportRequest::BlobImport>()->proxyHash,
      largeBlob);

  while (!enqueued.empty()) {
    auto expected = enqueued.front();
    enqueued.pop_front();

    auto request = queue.dequeue(1).at(0);

    EXPECT_EQ(
        expected, request->getRequest<HgImportRequest::BlobImport>()->hash);

    folly::Try<std::unique_ptr<Blob>> blob = folly::makeTryWith([expected]() {
      return std::make_unique<Blob>(expected, folly::IOBuf{});
    });
    queue.markImportAsFinished<Blob>(
        request->getRequest<HgImportRequest::BlobImport>()->proxyHash, blob);
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

      EXPECT_EQ(
          std::nullopt,
          queue.checkImportInProgress<Blob>(
              request.getRequest<HgImportRequest::BlobImport>()->proxyHash,
              ImportPriority(ImportPriorityKind::Normal, 0)));

      XLOG(INFO) << "enqueuing blob:" << hash;

      queue.enqueue(std::move(request));
      enqueued_blob.emplace(hash);
    }

    auto [hash, request] = makeTreeImportRequest(
        ImportPriority(ImportPriorityKind::Normal, 0), pendingImportWatches);

    EXPECT_EQ(
        std::nullopt,
        queue.checkImportInProgress<Tree>(
            request.getRequest<HgImportRequest::TreeImport>()->proxyHash,
            ImportPriority(ImportPriorityKind::Normal, 0)));

    XLOG(INFO) << "enqueuing tree:" << hash;
    queue.enqueue(std::move(request));
  }

  auto dequeued = queue.dequeue(20);
  EXPECT_EQ(dequeued.size(), 10);
  for (int i = 0; i < 10; i++) {
    auto dequeuedRequest = dequeued.at(i);

    EXPECT_TRUE(
        enqueued_blob.find(
            dequeuedRequest->getRequest<HgImportRequest::BlobImport>()->hash) !=
        enqueued_blob.end());

    folly::Try<std::unique_ptr<Blob>> blob = folly::makeTryWith(
        [hash = dequeuedRequest->getRequest<HgImportRequest::BlobImport>()
                    ->hash]() {
          return std::make_unique<Blob>(hash, folly::IOBuf{});
        });
    queue.markImportAsFinished<Blob>(
        dequeuedRequest->getRequest<HgImportRequest::BlobImport>()->proxyHash,
        blob);
  }
}

TEST(HgImportRequestQueueTest, duplicateRequestBeforeEnqueue) {
  auto queue = HgImportRequestQueue{};
  std::vector<Hash> enqueued;
  RequestMetricsScope::LockedRequestWatchList pendingImportWatches;

  auto hgRevHash = uniqueHash();
  auto proxyHash = HgProxyHash{RelativePath{"some_blob"}, hgRevHash};

  auto [hash, request] = makeBlobImportRequestWithHash(
      ImportPriority(ImportPriorityKind::Normal, 5),
      proxyHash,
      pendingImportWatches);

  auto [hash2, request2] = makeBlobImportRequestWithHash(
      ImportPriority(ImportPriorityKind::Normal, 5),
      proxyHash,
      pendingImportWatches);

  EXPECT_EQ(
      std::nullopt,
      queue.checkImportInProgress<Blob>(
          request.getRequest<HgImportRequest::BlobImport>()->proxyHash,
          ImportPriority(ImportPriorityKind::Normal, 5)));

  // duplicate request
  EXPECT_NE(
      std::nullopt,
      queue.checkImportInProgress<Blob>(
          request2.getRequest<HgImportRequest::BlobImport>()->proxyHash,
          ImportPriority(ImportPriorityKind::Normal, 5)));

  queue.enqueue(std::move(request));
  enqueued.push_back(hash);

  auto expected = enqueued.back();
  enqueued.pop_back();
  auto dequeuedRequest = queue.dequeue(1).at(0);
  EXPECT_EQ(
      1,
      dequeuedRequest->getRequest<HgImportRequest::BlobImport>()
          ->promises.size());
  EXPECT_EQ(
      expected,
      dequeuedRequest->getRequest<HgImportRequest::BlobImport>()->hash);

  folly::Try<std::unique_ptr<Blob>> blob =
      folly::makeTryWith([hash = proxyHash.sha1()]() {
        return std::make_unique<Blob>(hash, folly::IOBuf{});
      });
  queue.markImportAsFinished<Blob>(
      dequeuedRequest->getRequest<HgImportRequest::BlobImport>()->proxyHash,
      blob);
}

TEST(HgImportRequestQueueTest, duplicateRequestAfterEnqueue) {
  auto queue = HgImportRequestQueue{};
  std::vector<Hash> enqueued;
  RequestMetricsScope::LockedRequestWatchList pendingImportWatches;

  auto hgRevHash = uniqueHash();
  auto proxyHash = HgProxyHash{RelativePath{"some_blob"}, hgRevHash};

  auto [hash, request] = makeBlobImportRequestWithHash(
      ImportPriority(ImportPriorityKind::Normal, 5),
      proxyHash,
      pendingImportWatches);

  auto [hash2, request2] = makeBlobImportRequestWithHash(
      ImportPriority(ImportPriorityKind::Normal, 5),
      proxyHash,
      pendingImportWatches);

  EXPECT_EQ(
      std::nullopt,
      queue.checkImportInProgress<Blob>(
          request.getRequest<HgImportRequest::BlobImport>()->proxyHash,
          ImportPriority(ImportPriorityKind::Normal, 5)));

  queue.enqueue(std::move(request));
  enqueued.push_back(hash);

  // duplicate request
  EXPECT_NE(
      std::nullopt,
      queue.checkImportInProgress<Blob>(
          request2.getRequest<HgImportRequest::BlobImport>()->proxyHash,
          ImportPriority(ImportPriorityKind::Normal, 5)));

  auto expected = enqueued.back();
  enqueued.pop_back();
  auto dequeuedRequest = queue.dequeue(1).at(0);
  EXPECT_EQ(
      1,
      dequeuedRequest->getRequest<HgImportRequest::BlobImport>()
          ->promises.size());
  EXPECT_EQ(
      expected,
      dequeuedRequest->getRequest<HgImportRequest::BlobImport>()->hash);

  folly::Try<std::unique_ptr<Blob>> blob =
      folly::makeTryWith([hash = proxyHash.sha1()]() {
        return std::make_unique<Blob>(hash, folly::IOBuf{});
      });
  queue.markImportAsFinished<Blob>(
      dequeuedRequest->getRequest<HgImportRequest::BlobImport>()->proxyHash,
      blob);
}

TEST(HgImportRequestQueueTest, duplicateRequestAfterDequeue) {
  auto queue = HgImportRequestQueue{};
  std::vector<Hash> enqueued;
  RequestMetricsScope::LockedRequestWatchList pendingImportWatches;

  auto hgRevHash = uniqueHash();
  auto proxyHash = HgProxyHash{RelativePath{"some_blob"}, hgRevHash};

  auto [hash, request] = makeBlobImportRequestWithHash(
      ImportPriority(ImportPriorityKind::Normal, 5),
      proxyHash,
      pendingImportWatches);

  auto [hash2, request2] = makeBlobImportRequestWithHash(
      ImportPriority(ImportPriorityKind::Normal, 5),
      proxyHash,
      pendingImportWatches);

  EXPECT_EQ(
      std::nullopt,
      queue.checkImportInProgress<Blob>(
          request.getRequest<HgImportRequest::BlobImport>()->proxyHash,
          ImportPriority(ImportPriorityKind::Normal, 5)));

  queue.enqueue(std::move(request));
  enqueued.push_back(hash);

  auto expected = enqueued.back();
  enqueued.pop_back();
  auto dequeuedRequest = queue.dequeue(1).at(0);
  EXPECT_EQ(
      expected,
      dequeuedRequest->getRequest<HgImportRequest::BlobImport>()->hash);

  // duplicate request
  EXPECT_NE(
      std::nullopt,
      queue.checkImportInProgress<Blob>(
          request2.getRequest<HgImportRequest::BlobImport>()->proxyHash,
          ImportPriority(ImportPriorityKind::Normal, 5)));

  EXPECT_EQ(
      1,
      dequeuedRequest->getRequest<HgImportRequest::BlobImport>()
          ->promises.size());

  folly::Try<std::unique_ptr<Blob>> blob =
      folly::makeTryWith([hash = proxyHash.sha1()]() {
        return std::make_unique<Blob>(hash, folly::IOBuf{});
      });
  queue.markImportAsFinished<Blob>(
      dequeuedRequest->getRequest<HgImportRequest::BlobImport>()->proxyHash,
      blob);
}

TEST(HgImportRequestQueueTest, duplicateRequestAfterMarkedDone) {
  auto queue = HgImportRequestQueue{};
  std::vector<Hash> enqueued;
  RequestMetricsScope::LockedRequestWatchList pendingImportWatches;

  auto hgRevHash = uniqueHash();
  auto proxyHash = HgProxyHash{RelativePath{"some_blob"}, hgRevHash};

  auto [hash, request] = makeBlobImportRequestWithHash(
      ImportPriority(ImportPriorityKind::Normal, 5),
      proxyHash,
      pendingImportWatches);

  auto [hash2, request2] = makeBlobImportRequestWithHash(
      ImportPriority(ImportPriorityKind::Normal, 5),
      proxyHash,
      pendingImportWatches);

  EXPECT_EQ(
      std::nullopt,
      queue.checkImportInProgress<Blob>(
          request.getRequest<HgImportRequest::BlobImport>()->proxyHash,
          ImportPriority(ImportPriorityKind::Normal, 5)));

  queue.enqueue(std::move(request));
  enqueued.push_back(hash);

  auto expected = enqueued.back();
  enqueued.pop_back();
  auto dequeuedRequest = queue.dequeue(1).at(0);
  EXPECT_EQ(
      0,
      dequeuedRequest->getRequest<HgImportRequest::BlobImport>()
          ->promises.size());
  EXPECT_EQ(
      expected,
      dequeuedRequest->getRequest<HgImportRequest::BlobImport>()->hash);

  folly::Try<std::unique_ptr<Blob>> blob =
      folly::makeTryWith([hash = proxyHash.sha1()]() {
        return std::make_unique<Blob>(hash, folly::IOBuf{});
      });
  queue.markImportAsFinished<Blob>(
      dequeuedRequest->getRequest<HgImportRequest::BlobImport>()->proxyHash,
      blob);

  EXPECT_EQ(
      std::nullopt,
      queue.checkImportInProgress<Blob>(
          request2.getRequest<HgImportRequest::BlobImport>()->proxyHash,
          ImportPriority(ImportPriorityKind::Normal, 5)));
}

TEST(HgImportRequestQueueTest, multipleDuplicateRequests) {
  auto queue = HgImportRequestQueue{};
  std::vector<Hash> enqueued;
  RequestMetricsScope::LockedRequestWatchList pendingImportWatches;

  auto hgRevHash = uniqueHash();
  auto proxyHash = HgProxyHash{RelativePath{"some_blob"}, hgRevHash};

  auto [hash, request] = makeBlobImportRequestWithHash(
      ImportPriority(ImportPriorityKind::Normal, 5),
      proxyHash,
      pendingImportWatches);

  auto [hash2, request2] = makeBlobImportRequestWithHash(
      ImportPriority(ImportPriorityKind::Normal, 5),
      proxyHash,
      pendingImportWatches);

  auto [hash3, request3] = makeBlobImportRequestWithHash(
      ImportPriority(ImportPriorityKind::Normal, 5),
      proxyHash,
      pendingImportWatches);

  auto [hash4, request4] = makeBlobImportRequestWithHash(
      ImportPriority(ImportPriorityKind::Normal, 5),
      proxyHash,
      pendingImportWatches);

  EXPECT_EQ(
      std::nullopt,
      queue.checkImportInProgress<Blob>(
          request.getRequest<HgImportRequest::BlobImport>()->proxyHash,
          ImportPriority(ImportPriorityKind::Normal, 5)));

  // duplicate request
  EXPECT_NE(
      std::nullopt,
      queue.checkImportInProgress<Blob>(
          request2.getRequest<HgImportRequest::BlobImport>()->proxyHash,
          ImportPriority(ImportPriorityKind::Normal, 5)));

  queue.enqueue(std::move(request));
  enqueued.push_back(hash);

  // duplicate request
  EXPECT_NE(
      std::nullopt,
      queue.checkImportInProgress<Blob>(
          request3.getRequest<HgImportRequest::BlobImport>()->proxyHash,
          ImportPriority(ImportPriorityKind::Normal, 5)));

  auto expected = enqueued.back();
  enqueued.pop_back();
  auto dequeuedRequest = queue.dequeue(1).at(0);
  EXPECT_EQ(
      expected,
      dequeuedRequest->getRequest<HgImportRequest::BlobImport>()->hash);

  // duplicate request
  EXPECT_NE(
      std::nullopt,
      queue.checkImportInProgress<Blob>(
          request4.getRequest<HgImportRequest::BlobImport>()->proxyHash,
          ImportPriority(ImportPriorityKind::Normal, 5)));

  EXPECT_EQ(
      3,
      dequeuedRequest->getRequest<HgImportRequest::BlobImport>()
          ->promises.size());

  folly::Try<std::unique_ptr<Blob>> blob =
      folly::makeTryWith([hash = proxyHash.sha1()]() {
        return std::make_unique<Blob>(hash, folly::IOBuf{});
      });
  queue.markImportAsFinished<Blob>(
      dequeuedRequest->getRequest<HgImportRequest::BlobImport>()->proxyHash,
      blob);
}

TEST(HgImportRequestQueueTest, twoDuplicateRequestsDifferentPriority) {
  auto queue = HgImportRequestQueue{};
  std::vector<Hash> enqueued;
  RequestMetricsScope::LockedRequestWatchList pendingImportWatches;

  auto hgRevHash = uniqueHash();
  auto proxyHash = HgProxyHash{RelativePath{"some_blob"}, hgRevHash};

  auto [midPriHash, midPriRequest] = makeBlobImportRequestWithHash(
      ImportPriority(ImportPriorityKind::Normal, 6),
      proxyHash,
      pendingImportWatches);

  auto [lowPriHash, lowPriRequest] = makeBlobImportRequestWithHash(
      ImportPriority(ImportPriorityKind::Normal, 0),
      proxyHash,
      pendingImportWatches);

  for (int i = 1; i < 6; i++) {
    auto [hash, request] = makeBlobImportRequest(
        ImportPriority(ImportPriorityKind::Normal, i), pendingImportWatches);

    EXPECT_EQ(
        std::nullopt,
        queue.checkImportInProgress<Blob>(
            request.getRequest<HgImportRequest::BlobImport>()->proxyHash,
            ImportPriority(ImportPriorityKind::Normal, i)));

    queue.enqueue(std::move(request));
    enqueued.push_back(hash);
  }

  for (int i = 7; i < 11; i++) {
    auto [hash, request] = makeBlobImportRequest(
        ImportPriority(ImportPriorityKind::Normal, i), pendingImportWatches);

    EXPECT_EQ(
        std::nullopt,
        queue.checkImportInProgress<Blob>(
            request.getRequest<HgImportRequest::BlobImport>()->proxyHash,
            ImportPriority(ImportPriorityKind::Normal, i)));

    queue.enqueue(std::move(request));
    enqueued.push_back(hash);
  }

  // first check the low pri, which will be marked "in flight"
  EXPECT_EQ(
      std::nullopt,
      queue.checkImportInProgress<Blob>(
          lowPriRequest.getRequest<HgImportRequest::BlobImport>()->proxyHash,
          ImportPriority(ImportPriorityKind::Normal, 0)));

  // now check the mid pri, which will be turned away, but we expect the
  // priority to be respected
  EXPECT_NE(
      std::nullopt,
      queue.checkImportInProgress<Blob>(
          midPriRequest.getRequest<HgImportRequest::BlobImport>()->proxyHash,
          ImportPriority(ImportPriorityKind::Normal, 6)));

  queue.enqueue(std::move(lowPriRequest));

  // Now lets dequeue everything and make sure the smallHash now has middle
  // priority. We need to walk through the enqueued list backwards since we
  // enqueued in increasing priority.
  for (int i = 10; i > 6; i--) {
    auto expected = enqueued.back();
    enqueued.pop_back();

    auto request = queue.dequeue(1).at(0);

    EXPECT_EQ(
        expected, request->getRequest<HgImportRequest::BlobImport>()->hash);

    folly::Try<std::unique_ptr<Blob>> blob = folly::makeTryWith([expected]() {
      return std::make_unique<Blob>(expected, folly::IOBuf{});
    });
    queue.markImportAsFinished<Blob>(
        request->getRequest<HgImportRequest::BlobImport>()->proxyHash, blob);
  }

  auto expLowPri = queue.dequeue(1).at(0);

  EXPECT_EQ(
      lowPriHash, expLowPri->getRequest<HgImportRequest::BlobImport>()->hash);

  folly::Try<std::unique_ptr<Blob>> blob =
      folly::makeTryWith([lowPriHash = lowPriHash]() {
        return std::make_unique<Blob>(lowPriHash, folly::IOBuf{});
      });
  queue.markImportAsFinished<Blob>(
      expLowPri->getRequest<HgImportRequest::BlobImport>()->proxyHash, blob);

  for (int i = 5; i > 0; i--) {
    auto expected = enqueued.back();
    enqueued.pop_back();

    auto request = queue.dequeue(1).at(0);

    EXPECT_EQ(
        expected, request->getRequest<HgImportRequest::BlobImport>()->hash);

    folly::Try<std::unique_ptr<Blob>> expBlob =
        folly::makeTryWith([expected]() {
          return std::make_unique<Blob>(expected, folly::IOBuf{});
        });
    queue.markImportAsFinished<Blob>(
        request->getRequest<HgImportRequest::BlobImport>()->proxyHash, expBlob);
  }
}
