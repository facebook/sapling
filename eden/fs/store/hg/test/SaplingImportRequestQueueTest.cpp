/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/Try.h>
#include <folly/logging/xlog.h>
#include <gtest/gtest.h>
#include <array>
#include <memory>

#include "eden/common/telemetry/RequestMetricsScope.h"
#include "eden/common/utils/IDGen.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/TestOps.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/ImportPriority.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/store/hg/HgProxyHash.h"
#include "eden/fs/store/hg/SaplingImportRequest.h"
#include "eden/fs/store/hg/SaplingImportRequestQueue.h"

using namespace facebook::eden;

struct SaplingImportRequestQueueTest : ::testing::Test {
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

std::pair<ObjectId, std::shared_ptr<SaplingImportRequest>>
makeBlobImportRequest(ImportPriority priority) {
  auto hgRevHash = uniqueHash();
  auto proxyHash = HgProxyHash{RelativePath{"some_blob"}, hgRevHash};
  auto id = ObjectId{proxyHash.getValue()};
  auto requestContext = ObjectFetchContext::getNullContext();
  auto request = SaplingImportRequest::makeBlobImportRequest(
      id, std::move(proxyHash), requestContext);
  request->setPriority(priority);
  return std::make_pair(id, request);
}

std::pair<ObjectId, std::shared_ptr<SaplingImportRequest>>
makeBlobImportRequestWithHash(ImportPriority priority, HgProxyHash proxyHash) {
  auto id = ObjectId{proxyHash.getValue()};
  auto requestContext = ObjectFetchContext::getNullContext();
  auto request = SaplingImportRequest::makeBlobImportRequest(
      id, std::move(proxyHash), requestContext);
  request->setPriority(priority);
  return std::make_pair(id, request);
}

std::pair<ObjectId, std::shared_ptr<SaplingImportRequest>>
makeBlobAuxImportRequestWithHash(
    ImportPriority priority,
    HgProxyHash proxyHash) {
  auto id = ObjectId{proxyHash.getValue()};
  auto requestContext = ObjectFetchContext::getNullContext();
  auto request = SaplingImportRequest::makeBlobAuxImportRequest(
      id, std::move(proxyHash), requestContext);
  request->setPriority(priority);
  return std::make_pair(id, request);
}

std::pair<ObjectId, std::shared_ptr<SaplingImportRequest>>
makeTreeImportRequest(ImportPriority priority) {
  auto hgRevHash = uniqueHash();
  auto proxyHash = HgProxyHash{RelativePath{"some_tree"}, hgRevHash};
  auto id = ObjectId{proxyHash.getValue()};
  auto requestContext = ObjectFetchContext::getNullContext();
  auto request = SaplingImportRequest::makeTreeImportRequest(
      id, std::move(proxyHash), requestContext);
  request->setPriority(priority);
  return std::make_pair(id, request);
}

ObjectId insertBlobImportRequest(
    SaplingImportRequestQueue& queue,
    ImportPriority priority) {
  auto [id, request] = makeBlobImportRequest(priority);
  XLOGF(INFO, "enqueuing blob:{}", id);
  queue.enqueueBlob(std::move(request));
  return id;
}

ObjectId insertTreeImportRequest(
    SaplingImportRequestQueue& queue,
    ImportPriority priority) {
  auto [id, request] = makeTreeImportRequest(priority);
  XLOGF(INFO, "enqueuing tree:{}", id);
  queue.enqueueTree(std::move(request));
  return id;
}

TEST_F(SaplingImportRequestQueueTest, sameObjectIdDifferentType) {
  auto queue = SaplingImportRequestQueue{edenConfig};

  auto hgRevHash = uniqueHash();
  auto proxyHash = HgProxyHash{RelativePath{"some_blob"}, hgRevHash};

  auto [blobHash, blobRequest] = makeBlobImportRequestWithHash(
      ImportPriority(ImportPriority::Class::Normal, 1), proxyHash);
  auto [blobAuxHash, blobAuxRequest] = makeBlobAuxImportRequestWithHash(
      ImportPriority(ImportPriority::Class::Normal, 1), proxyHash);

  queue.enqueueBlob(std::move(blobRequest));
  queue.enqueueBlobAux(std::move(blobAuxRequest));

  auto request1 = queue.dequeue().at(0);
  EXPECT_NE(request1, nullptr);

  auto request2 = queue.dequeue().at(0);
  EXPECT_NE(request2, nullptr);
}

TEST_F(SaplingImportRequestQueueTest, getRequestByPriority) {
  auto queue = SaplingImportRequestQueue{edenConfig};
  std::vector<ObjectId> enqueued;

  for (int i = 0; i < 10; i++) {
    auto [id, request] =
        makeBlobImportRequest(ImportPriority(ImportPriority::Class::Normal, i));

    queue.enqueueBlob(std::move(request));
    enqueued.push_back(id);
  }

  auto [smallId, smallRequest] =
      makeBlobImportRequest(ImportPriority(ImportPriority::Class::Low, 0));

  queue.enqueueBlob(std::move(smallRequest));

  // the queue should give requests in the reverse order of pushing
  while (!enqueued.empty()) {
    auto expected = enqueued.back();
    enqueued.pop_back();
    auto request = queue.dequeue().at(0);
    EXPECT_EQ(
        expected, request->getRequest<SaplingImportRequest::BlobImport>()->id);

    auto blob = folly::makeTryWith([expected]() {
      return std::make_shared<BlobPtr::element_type>(folly::IOBuf{});
    });

    queue.markImportAsFinished<BlobPtr::element_type>(
        request->getRequest<SaplingImportRequest::BlobImport>()->id, blob);
  }

  auto smallRequestDequeue = queue.dequeue().at(0);
  EXPECT_EQ(
      smallId,
      smallRequestDequeue->getRequest<SaplingImportRequest::BlobImport>()->id);

  folly::Try<BlobPtr> smallBlob = folly::makeTryWith(
      [] { return std::make_shared<BlobPtr::element_type>(folly::IOBuf{}); });

  queue.markImportAsFinished<BlobPtr::element_type>(
      smallRequestDequeue->getRequest<SaplingImportRequest::BlobImport>()->id,
      smallBlob);
}

TEST_F(SaplingImportRequestQueueTest, getRequestByPriorityReverse) {
  auto queue = SaplingImportRequestQueue{edenConfig};
  std::deque<ObjectId> enqueued;

  for (int i = 0; i < 10; i++) {
    auto [id, request] = makeBlobImportRequest(
        ImportPriority(ImportPriority::Class::Normal, 10 - i));

    queue.enqueueBlob(std::move(request));
    enqueued.push_back(id);
  }

  auto [largeId, largeRequest] =
      makeBlobImportRequest(ImportPriority{ImportPriority::Class::High});

  queue.enqueueBlob(std::move(largeRequest));

  auto largeIdDequeue = queue.dequeue().at(0);
  EXPECT_EQ(
      largeId,
      largeIdDequeue->getRequest<SaplingImportRequest::BlobImport>()->id);

  folly::Try<BlobPtr> largeBlob = folly::makeTryWith(
      [] { return std::make_shared<BlobPtr::element_type>(folly::IOBuf{}); });
  queue.markImportAsFinished<BlobPtr::element_type>(
      largeIdDequeue->getRequest<SaplingImportRequest::BlobImport>()->id,
      largeBlob);

  while (!enqueued.empty()) {
    auto expected = enqueued.front();
    enqueued.pop_front();

    auto request = queue.dequeue().at(0);

    EXPECT_EQ(
        expected, request->getRequest<SaplingImportRequest::BlobImport>()->id);

    auto blob = folly::makeTryWith(
        [] { return std::make_shared<BlobPtr::element_type>(folly::IOBuf{}); });
    queue.markImportAsFinished<BlobPtr::element_type>(
        request->getRequest<SaplingImportRequest::BlobImport>()->id, blob);
  }
}

TEST_F(SaplingImportRequestQueueTest, mixedPriority) {
  auto queue = SaplingImportRequestQueue{edenConfig};
  std::set<ObjectId> enqueued_blob;
  std::set<ObjectId> enqueued_tree;

  for (int i = 0; i < 10; i++) {
    {
      auto id = insertBlobImportRequest(
          queue, ImportPriority(ImportPriority::Class::Normal, i));
      enqueued_blob.emplace(id);
    }
    auto id = insertTreeImportRequest(
        queue, ImportPriority(ImportPriority::Class::Normal, 10 - i));
    enqueued_tree.emplace(id);
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
            dequeuedRequest->getRequest<SaplingImportRequest::TreeImport>()
                ->id) != enqueued_tree.end());
    EXPECT_TRUE(
        dequeuedRequest->getPriority().value() ==
        ImportPriority(ImportPriority::Class::Normal, 10 - i)
            .value()); // assert tree requests of priority 10 and 9

    auto tree = folly::makeTryWith(
        [id = dequeuedRequest->getRequest<SaplingImportRequest::TreeImport>()
                  ->id]() {
          return std::make_shared<TreePtr::element_type>(
              Tree::container{kPathMapDefaultCaseSensitive}, id);
        });
    queue.markImportAsFinished<TreePtr::element_type>(
        dequeuedRequest->getRequest<SaplingImportRequest::TreeImport>()->id,
        tree);
  }

  // Pre dequeue, queue has tree requests from priority 1 to 8 and blob
  // requests from priority 0 to 9.
  auto dequeuedBlob = queue.dequeue();
  EXPECT_EQ(dequeuedBlob.size(), 3);
  for (int i = 0; i < 3; i++) {
    auto dequeuedRequest = dequeuedBlob.at(i);
    EXPECT_TRUE(
        enqueued_blob.find(
            dequeuedRequest->getRequest<SaplingImportRequest::BlobImport>()
                ->id) != enqueued_blob.end());
    EXPECT_TRUE(
        dequeuedRequest->getPriority().value() ==
        ImportPriority(ImportPriority::Class::Normal, 9 - i)
            .value()); // assert blob requests of priority 9, 8, and 7

    auto blob = folly::makeTryWith(
        [] { return std::make_shared<BlobPtr::element_type>(folly::IOBuf{}); });
    queue.markImportAsFinished<BlobPtr::element_type>(
        dequeuedRequest->getRequest<SaplingImportRequest::BlobImport>()->id,
        blob);
  }
}

TEST_F(SaplingImportRequestQueueTest, getMultipleRequests) {
  auto queue = SaplingImportRequestQueue{edenConfig};
  std::set<ObjectId> enqueued_blob;
  std::set<ObjectId> enqueued_tree;

  for (int i = 0; i < 10; i++) {
    {
      auto id = insertBlobImportRequest(
          queue, ImportPriority{ImportPriority::Class::Normal});
      enqueued_blob.emplace(id);
    }
    auto id = insertTreeImportRequest(
        queue, ImportPriority{ImportPriority::Class::Normal});
    enqueued_tree.emplace(id);
  }

  rawEdenConfig->importBatchSizeTree.setValue(
      10, ConfigSourceType::UserConfig, true);
  auto dequeuedTree = queue.dequeue();
  EXPECT_EQ(dequeuedTree.size(), 10);
  for (int i = 0; i < 10; i++) {
    auto dequeuedRequest = dequeuedTree.at(i);

    EXPECT_TRUE(
        enqueued_tree.find(
            dequeuedRequest->getRequest<SaplingImportRequest::TreeImport>()
                ->id) != enqueued_tree.end());

    auto tree = folly::makeTryWith(
        [id = dequeuedRequest->getRequest<SaplingImportRequest::TreeImport>()
                  ->id]() {
          return std::make_shared<TreePtr::element_type>(
              Tree::container{kPathMapDefaultCaseSensitive}, id);
        });
    queue.markImportAsFinished<TreePtr::element_type>(
        dequeuedRequest->getRequest<SaplingImportRequest::TreeImport>()->id,
        tree);
  }

  rawEdenConfig->importBatchSize.setValue(
      20, ConfigSourceType::UserConfig, true);
  auto dequeuedBlob = queue.dequeue();
  EXPECT_EQ(dequeuedBlob.size(), 10);
  for (int i = 0; i < 10; i++) {
    auto dequeuedRequest = dequeuedBlob.at(i);

    EXPECT_TRUE(
        enqueued_blob.find(
            dequeuedRequest->getRequest<SaplingImportRequest::BlobImport>()
                ->id) != enqueued_blob.end());

    auto blob = folly::makeTryWith(
        [] { return std::make_shared<BlobPtr::element_type>(folly::IOBuf{}); });
    queue.markImportAsFinished<BlobPtr::element_type>(
        dequeuedRequest->getRequest<SaplingImportRequest::BlobImport>()->id,
        blob);
  }
}

TEST_F(SaplingImportRequestQueueTest, duplicateRequestAfterEnqueue) {
  auto queue = SaplingImportRequestQueue{edenConfig};
  std::vector<ObjectId> enqueued;

  auto hgRevHash = uniqueHash();
  auto proxyHash = HgProxyHash{RelativePath{"some_blob"}, hgRevHash};

  auto [id, request] = makeBlobImportRequestWithHash(
      ImportPriority(ImportPriority::Class::Normal, 5), proxyHash);

  auto [id2, request2] = makeBlobImportRequestWithHash(
      ImportPriority(ImportPriority::Class::Normal, 5), proxyHash);

  queue.enqueueBlob(std::move(request));
  enqueued.push_back(id);
  queue.enqueueBlob(std::move(request2));

  auto expected = enqueued.back();
  enqueued.pop_back();
  auto dequeuedRequest = queue.dequeue().at(0);
  EXPECT_EQ(
      1,
      dequeuedRequest->getRequest<SaplingImportRequest::BlobImport>()
          ->promises.size());
  EXPECT_EQ(
      expected,
      dequeuedRequest->getRequest<SaplingImportRequest::BlobImport>()->id);

  auto blob = folly::makeTryWith(
      [] { return std::make_shared<BlobPtr::element_type>(folly::IOBuf{}); });
  queue.markImportAsFinished<BlobPtr::element_type>(
      dequeuedRequest->getRequest<SaplingImportRequest::BlobImport>()->id,
      blob);
}

TEST_F(SaplingImportRequestQueueTest, duplicateRequestAfterDequeue) {
  auto queue = SaplingImportRequestQueue{edenConfig};
  std::vector<ObjectId> enqueued;

  auto hgRevHash = uniqueHash();
  auto proxyHash = HgProxyHash{RelativePath{"some_blob"}, hgRevHash};

  auto [id, request] = makeBlobImportRequestWithHash(
      ImportPriority(ImportPriority::Class::Normal, 5), proxyHash);

  auto [id2, request2] = makeBlobImportRequestWithHash(
      ImportPriority(ImportPriority::Class::Normal, 5), proxyHash);

  queue.enqueueBlob(std::move(request));
  enqueued.push_back(id);

  auto expected = enqueued.back();
  enqueued.pop_back();
  auto dequeuedRequest = queue.dequeue().at(0);
  EXPECT_EQ(
      expected,
      dequeuedRequest->getRequest<SaplingImportRequest::BlobImport>()->id);

  queue.enqueueBlob(std::move(request2));

  EXPECT_EQ(
      1,
      dequeuedRequest->getRequest<SaplingImportRequest::BlobImport>()
          ->promises.size());

  auto blob = folly::makeTryWith(
      [] { return std::make_shared<BlobPtr::element_type>(folly::IOBuf{}); });
  queue.markImportAsFinished<BlobPtr::element_type>(
      dequeuedRequest->getRequest<SaplingImportRequest::BlobImport>()->id,
      blob);
}

TEST_F(SaplingImportRequestQueueTest, duplicateRequestAfterMarkedDone) {
  auto queue = SaplingImportRequestQueue{edenConfig};
  std::vector<ObjectId> enqueued;

  auto hgRevHash = uniqueHash();
  auto proxyHash = HgProxyHash{RelativePath{"some_blob"}, hgRevHash};

  auto [id, request] = makeBlobImportRequestWithHash(
      ImportPriority(ImportPriority::Class::Normal, 5), proxyHash);

  auto [id2, request2] = makeBlobImportRequestWithHash(
      ImportPriority(ImportPriority::Class::Normal, 5), proxyHash);

  queue.enqueueBlob(std::move(request));
  enqueued.push_back(id);

  auto expected = enqueued.back();
  enqueued.pop_back();
  auto dequeuedRequest = queue.dequeue().at(0);
  EXPECT_EQ(
      0,
      dequeuedRequest->getRequest<SaplingImportRequest::BlobImport>()
          ->promises.size());
  EXPECT_EQ(
      expected,
      dequeuedRequest->getRequest<SaplingImportRequest::BlobImport>()->id);

  auto blob = folly::makeTryWith(
      [] { return std::make_shared<BlobPtr::element_type>(folly::IOBuf{}); });
  queue.markImportAsFinished<BlobPtr::element_type>(
      dequeuedRequest->getRequest<SaplingImportRequest::BlobImport>()->id,
      blob);
}

TEST_F(SaplingImportRequestQueueTest, multipleDuplicateRequests) {
  auto queue = SaplingImportRequestQueue{edenConfig};
  std::vector<ObjectId> enqueued;

  auto hgRevHash = uniqueHash();
  auto proxyHash = HgProxyHash{RelativePath{"some_blob"}, hgRevHash};

  auto [id, request] = makeBlobImportRequestWithHash(
      ImportPriority(ImportPriority::Class::Normal, 5), proxyHash);

  auto [id2, request2] = makeBlobImportRequestWithHash(
      ImportPriority(ImportPriority::Class::Normal, 5), proxyHash);

  auto [id3, request3] = makeBlobImportRequestWithHash(
      ImportPriority(ImportPriority::Class::Normal, 5), proxyHash);

  auto [id4, request4] = makeBlobImportRequestWithHash(
      ImportPriority(ImportPriority::Class::Normal, 5), proxyHash);

  queue.enqueueBlob(std::move(request2));
  queue.enqueueBlob(std::move(request));
  enqueued.push_back(id);
  queue.enqueueBlob(std::move(request3));

  auto expected = enqueued.back();
  enqueued.pop_back();
  auto dequeuedRequest = queue.dequeue().at(0);
  EXPECT_EQ(
      expected,
      dequeuedRequest->getRequest<SaplingImportRequest::BlobImport>()->id);

  queue.enqueueBlob(std::move(request4));

  EXPECT_EQ(
      3,
      dequeuedRequest->getRequest<SaplingImportRequest::BlobImport>()
          ->promises.size());

  auto blob = folly::makeTryWith(
      [] { return std::make_shared<BlobPtr::element_type>(folly::IOBuf{}); });
  queue.markImportAsFinished<BlobPtr::element_type>(
      dequeuedRequest->getRequest<SaplingImportRequest::BlobImport>()->id,
      blob);
}

TEST_F(SaplingImportRequestQueueTest, twoDuplicateRequestsDifferentPriority) {
  auto queue = SaplingImportRequestQueue{edenConfig};
  std::vector<ObjectId> enqueued;

  auto hgRevHash = uniqueHash();
  auto proxyHash = HgProxyHash{RelativePath{"some_blob"}, hgRevHash};

  auto [midPriId, midPriRequest] = makeBlobImportRequestWithHash(
      ImportPriority(ImportPriority::Class::Normal, 6), proxyHash);

  auto [lowPriId, lowPriRequest] = makeBlobImportRequestWithHash(
      ImportPriority(ImportPriority::Class::Normal, 0), proxyHash);

  for (int i = 1; i < 6; i++) {
    auto [id, request] =
        makeBlobImportRequest(ImportPriority(ImportPriority::Class::Normal, i));

    queue.enqueueBlob(std::move(request));
    enqueued.push_back(id);
  }

  for (int i = 7; i < 11; i++) {
    auto [id, request] =
        makeBlobImportRequest(ImportPriority(ImportPriority::Class::Normal, i));

    queue.enqueueBlob(std::move(request));
    enqueued.push_back(id);
  }

  // first check the low pri, which will be marked "in flight"
  queue.enqueueBlob(std::move(lowPriRequest));

  // now check the mid pri, which will be turned away, but we expect the
  // priority to be respected
  queue.enqueueBlob(std::move(midPriRequest));

  // Now lets dequeue everything and make sure the smallId now has middle
  // priority. We need to walk through the enqueued list backwards since we
  // enqueued in increasing priority.
  for (int i = 10; i > 6; i--) {
    auto expected = enqueued.back();
    enqueued.pop_back();

    auto request = queue.dequeue().at(0);

    EXPECT_EQ(
        expected, request->getRequest<SaplingImportRequest::BlobImport>()->id);

    auto blob = folly::makeTryWith(
        [] { return std::make_shared<BlobPtr::element_type>(folly::IOBuf{}); });
    queue.markImportAsFinished<BlobPtr::element_type>(
        request->getRequest<SaplingImportRequest::BlobImport>()->id, blob);
  }

  auto expLowPri = queue.dequeue().at(0);

  EXPECT_EQ(
      lowPriId, expLowPri->getRequest<SaplingImportRequest::BlobImport>()->id);

  auto blob = folly::makeTryWith(
      [] { return std::make_shared<BlobPtr::element_type>(folly::IOBuf{}); });
  queue.markImportAsFinished<BlobPtr::element_type>(
      expLowPri->getRequest<SaplingImportRequest::BlobImport>()->id, blob);

  for (int i = 5; i > 0; i--) {
    auto expected = enqueued.back();
    enqueued.pop_back();

    auto request = queue.dequeue().at(0);

    EXPECT_EQ(
        expected, request->getRequest<SaplingImportRequest::BlobImport>()->id);

    auto expBlob = folly::makeTryWith(
        [] { return std::make_shared<BlobPtr::element_type>(folly::IOBuf{}); });
    queue.markImportAsFinished<BlobPtr::element_type>(
        request->getRequest<SaplingImportRequest::BlobImport>()->id, expBlob);
  }
}
