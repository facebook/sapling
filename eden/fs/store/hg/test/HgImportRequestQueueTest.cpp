/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/Try.h>
#include <folly/logging/xlog.h>
#include <folly/portability/GTest.h>
#include <array>
#include <memory>

#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/TestOps.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/ImportPriority.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/store/hg/HgImportRequest.h"
#include "eden/fs/store/hg/HgImportRequestQueue.h"
#include "eden/fs/store/hg/HgProxyHash.h"
#include "eden/fs/telemetry/RequestMetricsScope.h"
#include "eden/fs/utils/IDGen.h"

using namespace facebook::eden;

struct HgImportRequestQueueTest : ::testing::Test {
  std::shared_ptr<ReloadableConfig> edenConfig;
  std::shared_ptr<EdenConfig> rawEdenConfig;

  void SetUp() override {
    rawEdenConfig = EdenConfig::createTestEdenConfig();

    rawEdenConfig->importBatchSize.setValue(1, ConfigSourceType::Default, true);
    rawEdenConfig->importBatchSizeTree.setValue(
        1, ConfigSourceType::Default, true);

    edenConfig = std::make_shared<ReloadableConfig>(
        rawEdenConfig, ConfigReloadBehavior::NoReload);
  }
};

Hash20 uniqueHash() {
  std::array<uint8_t, Hash20::RAW_SIZE> bytes = {0};
  auto uid = generateUniqueID();
  std::memcpy(bytes.data(), &uid, sizeof(uid));
  return Hash20{bytes};
}

std::pair<ObjectId, std::shared_ptr<HgImportRequest>> makeBlobImportRequest(
    ImportPriority priority) {
  auto hgRevHash = uniqueHash();
  auto proxyHash = HgProxyHash{RelativePath{"some_blob"}, hgRevHash};
  auto hash = proxyHash.sha1();
  return std::make_pair(
      hash,
      HgImportRequest::makeBlobImportRequest(
          hash,
          std::move(proxyHash),
          priority,
          ObjectFetchContext::Cause::Unknown));
}

std::pair<ObjectId, std::shared_ptr<HgImportRequest>>
makeBlobImportRequestWithHash(ImportPriority priority, HgProxyHash proxyHash) {
  auto hash = proxyHash.sha1();
  return std::make_pair(
      hash,
      HgImportRequest::makeBlobImportRequest(
          hash,
          std::move(proxyHash),
          priority,
          ObjectFetchContext::Cause::Unknown));
}

std::pair<ObjectId, std::shared_ptr<HgImportRequest>> makeTreeImportRequest(
    ImportPriority priority) {
  auto hgRevHash = uniqueHash();
  auto proxyHash = HgProxyHash{RelativePath{"some_tree"}, hgRevHash};
  auto hash = proxyHash.sha1();
  return std::make_pair(
      hash,
      HgImportRequest::makeTreeImportRequest(
          hash,
          std::move(proxyHash),
          priority,
          ObjectFetchContext::Cause::Unknown));
}

ObjectId insertBlobImportRequest(
    HgImportRequestQueue& queue,
    ImportPriority priority) {
  auto [hash, request] = makeBlobImportRequest(priority);
  XLOG(INFO) << "enqueuing blob:" << hash;
  queue.enqueueBlob(std::move(request));
  return hash;
}

ObjectId insertTreeImportRequest(
    HgImportRequestQueue& queue,
    ImportPriority priority) {
  auto [hash, request] = makeTreeImportRequest(priority);
  XLOG(INFO) << "enqueuing tree:" << hash;
  queue.enqueueTree(std::move(request));
  return hash;
}

TEST_F(HgImportRequestQueueTest, getRequestByPriority) {
  auto queue = HgImportRequestQueue{edenConfig};
  std::vector<ObjectId> enqueued;

  for (int i = 0; i < 10; i++) {
    auto [hash, request] =
        makeBlobImportRequest(ImportPriority(ImportPriority::Class::Normal, i));

    queue.enqueueBlob(std::move(request));
    enqueued.push_back(hash);
  }

  auto [smallHash, smallRequest] =
      makeBlobImportRequest(ImportPriority(ImportPriority::Class::Low, 0));

  queue.enqueueBlob(std::move(smallRequest));

  // the queue should give requests in the reverse order of pushing
  while (!enqueued.empty()) {
    auto expected = enqueued.back();
    enqueued.pop_back();
    auto request = queue.dequeue().at(0);
    EXPECT_EQ(
        expected, request->getRequest<HgImportRequest::BlobImport>()->hash);

    folly::Try<std::unique_ptr<Blob>> blob = folly::makeTryWith([expected]() {
      return std::make_unique<Blob>(expected, folly::IOBuf{});
    });

    queue.markImportAsFinished<Blob>(
        request->getRequest<HgImportRequest::BlobImport>()->hash, blob);
  }

  auto smallRequestDequeue = queue.dequeue().at(0);
  EXPECT_EQ(
      smallHash,
      smallRequestDequeue->getRequest<HgImportRequest::BlobImport>()->hash);

  folly::Try<std::unique_ptr<Blob>> smallBlob =
      folly::makeTryWith([smallHash = smallHash]() {
        return std::make_unique<Blob>(smallHash, folly::IOBuf{});
      });

  queue.markImportAsFinished<Blob>(
      smallRequestDequeue->getRequest<HgImportRequest::BlobImport>()->hash,
      smallBlob);
}

TEST_F(HgImportRequestQueueTest, getRequestByPriorityReverse) {
  auto queue = HgImportRequestQueue{edenConfig};
  std::deque<ObjectId> enqueued;

  for (int i = 0; i < 10; i++) {
    auto [hash, request] = makeBlobImportRequest(
        ImportPriority(ImportPriority::Class::Normal, 10 - i));

    queue.enqueueBlob(std::move(request));
    enqueued.push_back(hash);
  }

  auto [largeHash, largeRequest] =
      makeBlobImportRequest(ImportPriority{ImportPriority::Class::High});

  queue.enqueueBlob(std::move(largeRequest));

  auto largeHashDequeue = queue.dequeue().at(0);
  EXPECT_EQ(
      largeHash,
      largeHashDequeue->getRequest<HgImportRequest::BlobImport>()->hash);

  folly::Try<std::unique_ptr<Blob>> largeBlob =
      folly::makeTryWith([largeHash = largeHash]() {
        return std::make_unique<Blob>(largeHash, folly::IOBuf{});
      });
  queue.markImportAsFinished<Blob>(
      largeHashDequeue->getRequest<HgImportRequest::BlobImport>()->hash,
      largeBlob);

  while (!enqueued.empty()) {
    auto expected = enqueued.front();
    enqueued.pop_front();

    auto request = queue.dequeue().at(0);

    EXPECT_EQ(
        expected, request->getRequest<HgImportRequest::BlobImport>()->hash);

    folly::Try<std::unique_ptr<Blob>> blob = folly::makeTryWith([expected]() {
      return std::make_unique<Blob>(expected, folly::IOBuf{});
    });
    queue.markImportAsFinished<Blob>(
        request->getRequest<HgImportRequest::BlobImport>()->hash, blob);
  }
}

TEST_F(HgImportRequestQueueTest, mixedPriority) {
  auto queue = HgImportRequestQueue{edenConfig};
  std::set<ObjectId> enqueued_blob;
  std::set<ObjectId> enqueued_tree;

  for (int i = 0; i < 10; i++) {
    {
      auto hash = insertBlobImportRequest(
          queue, ImportPriority(ImportPriority::Class::Normal, i));
      enqueued_blob.emplace(hash);
    }
    auto hash = insertTreeImportRequest(
        queue, ImportPriority(ImportPriority::Class::Normal, 10 - i));
    enqueued_tree.emplace(hash);
  }

  rawEdenConfig->importBatchSize.setValue(
      3, ConfigSourceType::UserConfig, true);
  rawEdenConfig->importBatchSizeTree.setValue(
      2, ConfigSourceType::UserConfig, true);

  // Pre dequeue, queue has tree requests from priority 1 to 10 and blob
  // requests from priority 0 to 9.
  auto dequeuedTree = queue.dequeue();
  EXPECT_EQ(dequeuedTree.size(), 2);
  for (int i = 0; i < 2; i++) {
    auto dequeuedRequest = dequeuedTree.at(i);
    EXPECT_TRUE(
        enqueued_tree.find(
            dequeuedRequest->getRequest<HgImportRequest::TreeImport>()->hash) !=
        enqueued_tree.end());
    EXPECT_TRUE(
        dequeuedRequest->getPriority().value() ==
        ImportPriority(ImportPriority::Class::Normal, 10 - i)
            .value()); // assert tree requests of priority 10 and 9

    folly::Try<std::unique_ptr<Tree>> tree = folly::makeTryWith(
        [hash = dequeuedRequest->getRequest<HgImportRequest::TreeImport>()
                    ->hash]() {
          return std::make_unique<Tree>(
              Tree::container{kPathMapDefaultCaseSensitive}, hash);
        });
    queue.markImportAsFinished<Tree>(
        dequeuedRequest->getRequest<HgImportRequest::TreeImport>()->hash, tree);
  }

  // Pre dequeue, queue has tree requests from priority 1 to 8 and blob
  // requests from priority 0 to 9.
  auto dequeuedBlob = queue.dequeue();
  EXPECT_EQ(dequeuedBlob.size(), 3);
  for (int i = 0; i < 3; i++) {
    auto dequeuedRequest = dequeuedBlob.at(i);
    EXPECT_TRUE(
        enqueued_blob.find(
            dequeuedRequest->getRequest<HgImportRequest::BlobImport>()->hash) !=
        enqueued_blob.end());
    EXPECT_TRUE(
        dequeuedRequest->getPriority().value() ==
        ImportPriority(ImportPriority::Class::Normal, 9 - i)
            .value()); // assert blob requests of priority 9, 8, and 7

    folly::Try<std::unique_ptr<Blob>> blob = folly::makeTryWith(
        [hash = dequeuedRequest->getRequest<HgImportRequest::BlobImport>()
                    ->hash]() {
          return std::make_unique<Blob>(hash, folly::IOBuf{});
        });
    queue.markImportAsFinished<Blob>(
        dequeuedRequest->getRequest<HgImportRequest::BlobImport>()->hash, blob);
  }
}

TEST_F(HgImportRequestQueueTest, getMultipleRequests) {
  auto queue = HgImportRequestQueue{edenConfig};
  std::set<ObjectId> enqueued_blob;
  std::set<ObjectId> enqueued_tree;

  for (int i = 0; i < 10; i++) {
    {
      auto hash = insertBlobImportRequest(
          queue, ImportPriority{ImportPriority::Class::Normal});
      enqueued_blob.emplace(hash);
    }
    auto hash = insertTreeImportRequest(
        queue, ImportPriority{ImportPriority::Class::Normal});
    enqueued_tree.emplace(hash);
  }

  rawEdenConfig->importBatchSizeTree.setValue(
      10, ConfigSourceType::UserConfig, true);
  auto dequeuedTree = queue.dequeue();
  EXPECT_EQ(dequeuedTree.size(), 10);
  for (int i = 0; i < 10; i++) {
    auto dequeuedRequest = dequeuedTree.at(i);

    EXPECT_TRUE(
        enqueued_tree.find(
            dequeuedRequest->getRequest<HgImportRequest::TreeImport>()->hash) !=
        enqueued_tree.end());

    folly::Try<std::unique_ptr<Tree>> tree = folly::makeTryWith(
        [hash = dequeuedRequest->getRequest<HgImportRequest::TreeImport>()
                    ->hash]() {
          return std::make_unique<Tree>(
              Tree::container{kPathMapDefaultCaseSensitive}, hash);
        });
    queue.markImportAsFinished<Tree>(
        dequeuedRequest->getRequest<HgImportRequest::TreeImport>()->hash, tree);
  }

  rawEdenConfig->importBatchSize.setValue(
      20, ConfigSourceType::UserConfig, true);
  auto dequeuedBlob = queue.dequeue();
  EXPECT_EQ(dequeuedBlob.size(), 10);
  for (int i = 0; i < 10; i++) {
    auto dequeuedRequest = dequeuedBlob.at(i);

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
        dequeuedRequest->getRequest<HgImportRequest::BlobImport>()->hash, blob);
  }
}

TEST_F(HgImportRequestQueueTest, duplicateRequestAfterEnqueue) {
  auto queue = HgImportRequestQueue{edenConfig};
  std::vector<ObjectId> enqueued;

  auto hgRevHash = uniqueHash();
  auto proxyHash = HgProxyHash{RelativePath{"some_blob"}, hgRevHash};

  auto [hash, request] = makeBlobImportRequestWithHash(
      ImportPriority(ImportPriority::Class::Normal, 5), proxyHash);

  auto [hash2, request2] = makeBlobImportRequestWithHash(
      ImportPriority(ImportPriority::Class::Normal, 5), proxyHash);

  queue.enqueueBlob(std::move(request));
  enqueued.push_back(hash);
  queue.enqueueBlob(std::move(request2));

  auto expected = enqueued.back();
  enqueued.pop_back();
  auto dequeuedRequest = queue.dequeue().at(0);
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
      dequeuedRequest->getRequest<HgImportRequest::BlobImport>()->hash, blob);
}

TEST_F(HgImportRequestQueueTest, duplicateRequestAfterDequeue) {
  auto queue = HgImportRequestQueue{edenConfig};
  std::vector<ObjectId> enqueued;

  auto hgRevHash = uniqueHash();
  auto proxyHash = HgProxyHash{RelativePath{"some_blob"}, hgRevHash};

  auto [hash, request] = makeBlobImportRequestWithHash(
      ImportPriority(ImportPriority::Class::Normal, 5), proxyHash);

  auto [hash2, request2] = makeBlobImportRequestWithHash(
      ImportPriority(ImportPriority::Class::Normal, 5), proxyHash);

  queue.enqueueBlob(std::move(request));
  enqueued.push_back(hash);

  auto expected = enqueued.back();
  enqueued.pop_back();
  auto dequeuedRequest = queue.dequeue().at(0);
  EXPECT_EQ(
      expected,
      dequeuedRequest->getRequest<HgImportRequest::BlobImport>()->hash);

  queue.enqueueBlob(std::move(request2));

  EXPECT_EQ(
      1,
      dequeuedRequest->getRequest<HgImportRequest::BlobImport>()
          ->promises.size());

  folly::Try<std::unique_ptr<Blob>> blob =
      folly::makeTryWith([hash = proxyHash.sha1()]() {
        return std::make_unique<Blob>(hash, folly::IOBuf{});
      });
  queue.markImportAsFinished<Blob>(
      dequeuedRequest->getRequest<HgImportRequest::BlobImport>()->hash, blob);
}

TEST_F(HgImportRequestQueueTest, duplicateRequestAfterMarkedDone) {
  auto queue = HgImportRequestQueue{edenConfig};
  std::vector<ObjectId> enqueued;

  auto hgRevHash = uniqueHash();
  auto proxyHash = HgProxyHash{RelativePath{"some_blob"}, hgRevHash};

  auto [hash, request] = makeBlobImportRequestWithHash(
      ImportPriority(ImportPriority::Class::Normal, 5), proxyHash);

  auto [hash2, request2] = makeBlobImportRequestWithHash(
      ImportPriority(ImportPriority::Class::Normal, 5), proxyHash);

  queue.enqueueBlob(std::move(request));
  enqueued.push_back(hash);

  auto expected = enqueued.back();
  enqueued.pop_back();
  auto dequeuedRequest = queue.dequeue().at(0);
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
      dequeuedRequest->getRequest<HgImportRequest::BlobImport>()->hash, blob);
}

TEST_F(HgImportRequestQueueTest, multipleDuplicateRequests) {
  auto queue = HgImportRequestQueue{edenConfig};
  std::vector<ObjectId> enqueued;

  auto hgRevHash = uniqueHash();
  auto proxyHash = HgProxyHash{RelativePath{"some_blob"}, hgRevHash};

  auto [hash, request] = makeBlobImportRequestWithHash(
      ImportPriority(ImportPriority::Class::Normal, 5), proxyHash);

  auto [hash2, request2] = makeBlobImportRequestWithHash(
      ImportPriority(ImportPriority::Class::Normal, 5), proxyHash);

  auto [hash3, request3] = makeBlobImportRequestWithHash(
      ImportPriority(ImportPriority::Class::Normal, 5), proxyHash);

  auto [hash4, request4] = makeBlobImportRequestWithHash(
      ImportPriority(ImportPriority::Class::Normal, 5), proxyHash);

  queue.enqueueBlob(std::move(request2));
  queue.enqueueBlob(std::move(request));
  enqueued.push_back(hash);
  queue.enqueueBlob(std::move(request3));

  auto expected = enqueued.back();
  enqueued.pop_back();
  auto dequeuedRequest = queue.dequeue().at(0);
  EXPECT_EQ(
      expected,
      dequeuedRequest->getRequest<HgImportRequest::BlobImport>()->hash);

  queue.enqueueBlob(std::move(request4));

  EXPECT_EQ(
      3,
      dequeuedRequest->getRequest<HgImportRequest::BlobImport>()
          ->promises.size());

  folly::Try<std::unique_ptr<Blob>> blob =
      folly::makeTryWith([hash = proxyHash.sha1()]() {
        return std::make_unique<Blob>(hash, folly::IOBuf{});
      });
  queue.markImportAsFinished<Blob>(
      dequeuedRequest->getRequest<HgImportRequest::BlobImport>()->hash, blob);
}

TEST_F(HgImportRequestQueueTest, twoDuplicateRequestsDifferentPriority) {
  auto queue = HgImportRequestQueue{edenConfig};
  std::vector<ObjectId> enqueued;

  auto hgRevHash = uniqueHash();
  auto proxyHash = HgProxyHash{RelativePath{"some_blob"}, hgRevHash};

  auto [midPriHash, midPriRequest] = makeBlobImportRequestWithHash(
      ImportPriority(ImportPriority::Class::Normal, 6), proxyHash);

  auto [lowPriHash, lowPriRequest] = makeBlobImportRequestWithHash(
      ImportPriority(ImportPriority::Class::Normal, 0), proxyHash);

  for (int i = 1; i < 6; i++) {
    auto [hash, request] =
        makeBlobImportRequest(ImportPriority(ImportPriority::Class::Normal, i));

    queue.enqueueBlob(std::move(request));
    enqueued.push_back(hash);
  }

  for (int i = 7; i < 11; i++) {
    auto [hash, request] =
        makeBlobImportRequest(ImportPriority(ImportPriority::Class::Normal, i));

    queue.enqueueBlob(std::move(request));
    enqueued.push_back(hash);
  }

  // first check the low pri, which will be marked "in flight"
  queue.enqueueBlob(std::move(lowPriRequest));

  // now check the mid pri, which will be turned away, but we expect the
  // priority to be respected
  queue.enqueueBlob(std::move(midPriRequest));

  // Now lets dequeue everything and make sure the smallHash now has middle
  // priority. We need to walk through the enqueued list backwards since we
  // enqueued in increasing priority.
  for (int i = 10; i > 6; i--) {
    auto expected = enqueued.back();
    enqueued.pop_back();

    auto request = queue.dequeue().at(0);

    EXPECT_EQ(
        expected, request->getRequest<HgImportRequest::BlobImport>()->hash);

    folly::Try<std::unique_ptr<Blob>> blob = folly::makeTryWith([expected]() {
      return std::make_unique<Blob>(expected, folly::IOBuf{});
    });
    queue.markImportAsFinished<Blob>(
        request->getRequest<HgImportRequest::BlobImport>()->hash, blob);
  }

  auto expLowPri = queue.dequeue().at(0);

  EXPECT_EQ(
      lowPriHash, expLowPri->getRequest<HgImportRequest::BlobImport>()->hash);

  folly::Try<std::unique_ptr<Blob>> blob =
      folly::makeTryWith([lowPriHash = lowPriHash]() {
        return std::make_unique<Blob>(lowPriHash, folly::IOBuf{});
      });
  queue.markImportAsFinished<Blob>(
      expLowPri->getRequest<HgImportRequest::BlobImport>()->hash, blob);

  for (int i = 5; i > 0; i--) {
    auto expected = enqueued.back();
    enqueued.pop_back();

    auto request = queue.dequeue().at(0);

    EXPECT_EQ(
        expected, request->getRequest<HgImportRequest::BlobImport>()->hash);

    folly::Try<std::unique_ptr<Blob>> expBlob =
        folly::makeTryWith([expected]() {
          return std::make_unique<Blob>(expected, folly::IOBuf{});
        });
    queue.markImportAsFinished<Blob>(
        request->getRequest<HgImportRequest::BlobImport>()->hash, expBlob);
  }
}
