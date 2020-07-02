/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/executors/QueuedImmediateExecutor.h>
#include <folly/test/TestUtils.h>
#include <gtest/gtest.h>

#include "eden/fs/store/MemoryLocalStore.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/telemetry/NullStructuredLogger.h"
#include "eden/fs/testharness/FakeBackingStore.h"
#include "eden/fs/testharness/LoggingFetchContext.h"
#include "eden/fs/testharness/StoredObject.h"

using namespace facebook::eden;
using namespace folly::string_piece_literals;
using namespace std::chrono_literals;

namespace {

struct ObjectStoreTest : ::testing::Test {
  void SetUp() override {
    localStore = std::make_shared<MemoryLocalStore>();
    backingStore = std::make_shared<FakeBackingStore>(localStore);
    stats = std::make_shared<EdenStats>();
    executor = &folly::QueuedImmediateExecutor::instance();
    objectStore = ObjectStore::create(
        localStore,
        backingStore,
        stats,
        executor,
        std::make_shared<ProcessNameCache>(),
        std::make_shared<NullStructuredLogger>());

    readyBlobId = putReadyBlob("readyblob");
    readyTreeId = putReadyTree();
  }

  Hash putReadyBlob(folly::StringPiece data) {
    StoredBlob* storedBlob = backingStore->putBlob(data);
    storedBlob->setReady();
    return storedBlob->get().getHash();
  }

  Hash putReadyTree() {
    StoredTree* storedTree = backingStore->putTree({});
    storedTree->setReady();
    return storedTree->get().getHash();
  }

  LoggingFetchContext context;
  std::shared_ptr<LocalStore> localStore;
  std::shared_ptr<FakeBackingStore> backingStore;
  std::shared_ptr<EdenStats> stats;
  std::shared_ptr<ObjectStore> objectStore;
  folly::QueuedImmediateExecutor* executor;

  Hash readyBlobId;
  Hash readyTreeId;
};

} // namespace

TEST_F(ObjectStoreTest, getBlob_tracks_backing_store_read) {
  objectStore->getBlob(readyBlobId, context).get(0ms);
  ASSERT_EQ(1, context.requests.size());
  auto& request = context.requests[0];
  EXPECT_EQ(ObjectFetchContext::Blob, request.type);
  EXPECT_EQ(readyBlobId, request.hash);
  EXPECT_EQ(ObjectFetchContext::FromBackingStore, request.origin);
}

TEST_F(ObjectStoreTest, getBlob_tracks_second_read_from_cache) {
  objectStore->getBlob(readyBlobId, context).get(0ms);
  objectStore->getBlob(readyBlobId, context).get(0ms);
  ASSERT_EQ(2, context.requests.size());
  auto& request = context.requests[1];
  EXPECT_EQ(ObjectFetchContext::Blob, request.type);
  EXPECT_EQ(readyBlobId, request.hash);
  EXPECT_EQ(ObjectFetchContext::FromDiskCache, request.origin);
}

TEST_F(ObjectStoreTest, getTree_tracks_backing_store_read) {
  objectStore->getTree(readyTreeId, context).get(0ms);
  ASSERT_EQ(1, context.requests.size());
  auto& request = context.requests[0];
  EXPECT_EQ(ObjectFetchContext::Tree, request.type);
  EXPECT_EQ(readyTreeId, request.hash);
  EXPECT_EQ(ObjectFetchContext::FromBackingStore, request.origin);
}

TEST_F(ObjectStoreTest, getTree_tracks_second_read_from_cache) {
  objectStore->getTree(readyTreeId, context).get(0ms);
  objectStore->getTree(readyTreeId, context).get(0ms);
  ASSERT_EQ(2, context.requests.size());
  auto& request = context.requests[1];
  EXPECT_EQ(ObjectFetchContext::Tree, request.type);
  EXPECT_EQ(readyTreeId, request.hash);
  EXPECT_EQ(ObjectFetchContext::FromDiskCache, request.origin);
}

TEST_F(ObjectStoreTest, getBlobSize_tracks_backing_store_read) {
  objectStore->getBlobSize(readyBlobId, context).get(0ms);
  ASSERT_EQ(1, context.requests.size());
  auto& request = context.requests[0];
  EXPECT_EQ(ObjectFetchContext::BlobMetadata, request.type);
  EXPECT_EQ(readyBlobId, request.hash);
  EXPECT_EQ(ObjectFetchContext::FromBackingStore, request.origin);
}

TEST_F(ObjectStoreTest, getBlobSize_tracks_second_read_from_cache) {
  objectStore->getBlobSize(readyBlobId, context).get(0ms);
  objectStore->getBlobSize(readyBlobId, context).get(0ms);
  ASSERT_EQ(2, context.requests.size());
  auto& request = context.requests[1];
  EXPECT_EQ(ObjectFetchContext::BlobMetadata, request.type);
  EXPECT_EQ(readyBlobId, request.hash);
  EXPECT_EQ(ObjectFetchContext::FromMemoryCache, request.origin);
}

TEST_F(ObjectStoreTest, getBlobSizeFromLocalStore) {
  auto data = "A"_sp;
  Hash id = putReadyBlob(data);

  // Get blob size from backing store, caches in local store
  objectStore->getBlobSize(id, context);
  // Clear backing store
  objectStore = ObjectStore::create(
      localStore,
      nullptr,
      stats,
      executor,
      std::make_shared<ProcessNameCache>(),
      std::make_shared<NullStructuredLogger>());

  size_t expectedSize = data.size();
  size_t size = objectStore->getBlobSize(id, context).get();
  EXPECT_EQ(expectedSize, size);
}

TEST_F(ObjectStoreTest, getBlobSizeFromBackingStore) {
  auto data = "A"_sp;
  Hash id = putReadyBlob(data);

  size_t expectedSize = data.size();
  size_t size = objectStore->getBlobSize(id, context).get();
  EXPECT_EQ(expectedSize, size);
}

TEST_F(ObjectStoreTest, getBlobSizeNotFound) {
  Hash id;

  EXPECT_THROW_RE(
      objectStore->getBlobSize(id, context).get(),
      std::domain_error,
      "blob .* not found");
}

TEST_F(ObjectStoreTest, getBlobSha1) {
  auto data = "A"_sp;
  Hash id = putReadyBlob(data);

  Hash expectedSha1 = Hash::sha1(data);
  Hash sha1 = objectStore->getBlobSha1(id, context).get();
  EXPECT_EQ(expectedSha1.toString(), sha1.toString());
}

TEST_F(ObjectStoreTest, getBlobSha1NotFound) {
  Hash id;

  EXPECT_THROW_RE(
      objectStore->getBlobSha1(id, context).get(),
      std::domain_error,
      "blob .* not found");
}

TEST_F(ObjectStoreTest, get_size_and_sha1_only_imports_blob_once) {
  objectStore->getBlobSize(readyBlobId, context).get(0ms);
  objectStore->getBlobSha1(readyBlobId, context).get(0ms);

  EXPECT_EQ(1, backingStore->getAccessCount(readyBlobId));
}
