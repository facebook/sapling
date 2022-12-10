/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/portability/GTest.h>
#include <folly/test/TestUtils.h>

#include "eden/fs/model/TestOps.h"
#include "eden/fs/store/LocalStoreCachedBackingStore.h"
#include "eden/fs/store/MemoryLocalStore.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/telemetry/NullStructuredLogger.h"
#include "eden/fs/testharness/FakeBackingStore.h"
#include "eden/fs/testharness/LoggingFetchContext.h"
#include "eden/fs/testharness/StoredObject.h"
#include "eden/fs/utils/ImmediateFuture.h"

using namespace facebook::eden;
using namespace folly::string_piece_literals;
using namespace std::chrono_literals;

namespace {

constexpr size_t kTreeCacheMaximumSize = 1000; // bytes
constexpr size_t kTreeCacheMinimumEntries = 0;

struct ObjectStoreTest : ::testing::Test {
  void SetUp() override {
    std::shared_ptr<EdenConfig> rawEdenConfig{
        EdenConfig::createTestEdenConfig()};
    rawEdenConfig->inMemoryTreeCacheSize.setValue(
        kTreeCacheMaximumSize, ConfigSourceType::Default, true);
    rawEdenConfig->inMemoryTreeCacheMinElements.setValue(
        kTreeCacheMinimumEntries, ConfigSourceType::Default, true);
    auto edenConfig = std::make_shared<ReloadableConfig>(
        rawEdenConfig, ConfigReloadBehavior::NoReload);
    treeCache = TreeCache::create(edenConfig);
    localStore = std::make_shared<MemoryLocalStore>();
    stats = std::make_shared<EdenStats>();
    fakeBackingStore = std::make_shared<FakeBackingStore>();
    backingStore = std::make_shared<LocalStoreCachedBackingStore>(
        fakeBackingStore, localStore, stats);
    objectStore = ObjectStore::create(
        localStore,
        backingStore,
        treeCache,
        stats,
        std::make_shared<ProcessNameCache>(),
        std::make_shared<NullStructuredLogger>(),
        EdenConfig::createTestEdenConfig(),
        kPathMapDefaultCaseSensitive);

    readyBlobId = putReadyBlob("readyblob");
    readyTreeId = putReadyTree();
  }

  ObjectId putReadyBlob(folly::StringPiece data) {
    StoredBlob* storedBlob = fakeBackingStore->putBlob(data);
    storedBlob->setReady();
    return storedBlob->get().getHash();
  }

  ObjectId putReadyTree() {
    StoredTree* storedTree = fakeBackingStore->putTree({});
    storedTree->setReady();
    return storedTree->get().getHash();
  }

  RefPtr<LoggingFetchContext> loggingContext =
      makeRefPtr<LoggingFetchContext>();
  const ObjectFetchContextPtr& context =
      loggingContext.as<ObjectFetchContext>();
  std::shared_ptr<LocalStore> localStore;
  std::shared_ptr<FakeBackingStore> fakeBackingStore;
  std::shared_ptr<BackingStore> backingStore;
  std::shared_ptr<TreeCache> treeCache;
  std::shared_ptr<EdenStats> stats;
  std::shared_ptr<ObjectStore> objectStore;

  ObjectId readyBlobId;
  ObjectId readyTreeId;
};

} // namespace

TEST_F(ObjectStoreTest, getBlob_tracks_backing_store_read) {
  objectStore->getBlob(readyBlobId, context).get(0ms);
  ASSERT_EQ(1, loggingContext->requests.size());
  auto& request = loggingContext->requests[0];
  EXPECT_EQ(ObjectFetchContext::Blob, request.type);
  EXPECT_EQ(readyBlobId, request.hash);
  EXPECT_EQ(ObjectFetchContext::FromNetworkFetch, request.origin);
}

TEST_F(ObjectStoreTest, getBlob_tracks_second_read_from_cache) {
  objectStore->getBlob(readyBlobId, context).get(0ms);
  objectStore->getBlob(readyBlobId, context).get(0ms);
  ASSERT_EQ(2, loggingContext->requests.size());
  auto& request = loggingContext->requests[1];
  EXPECT_EQ(ObjectFetchContext::Blob, request.type);
  EXPECT_EQ(readyBlobId, request.hash);
  EXPECT_EQ(ObjectFetchContext::FromDiskCache, request.origin);
}

TEST_F(ObjectStoreTest, getTree_tracks_backing_store_read) {
  objectStore->getTree(readyTreeId, context).get(0ms);
  ASSERT_EQ(1, loggingContext->requests.size());
  auto& request = loggingContext->requests[0];
  EXPECT_EQ(ObjectFetchContext::Tree, request.type);
  EXPECT_EQ(readyTreeId, request.hash);
  EXPECT_EQ(ObjectFetchContext::FromNetworkFetch, request.origin);
}

TEST_F(ObjectStoreTest, getTree_tracks_second_read_from_cache) {
  objectStore->getTree(readyTreeId, context).get(0ms);
  objectStore->getTree(readyTreeId, context).get(0ms);
  ASSERT_EQ(2, loggingContext->requests.size());
  auto& request = loggingContext->requests[1];
  EXPECT_EQ(ObjectFetchContext::Tree, request.type);
  EXPECT_EQ(readyTreeId, request.hash);
  EXPECT_EQ(ObjectFetchContext::FromMemoryCache, request.origin);
}

TEST_F(ObjectStoreTest, getTree_tracks_second_read_from_local_store) {
  objectStore->getTree(readyTreeId, context).get(0ms);

  // clear the in memory cache so the tree can not be found here
  treeCache->clear();

  objectStore->getTree(readyTreeId, context).get(0ms);
  ASSERT_EQ(2, loggingContext->requests.size());
  auto& request = loggingContext->requests[1];
  EXPECT_EQ(ObjectFetchContext::Tree, request.type);
  EXPECT_EQ(readyTreeId, request.hash);
  EXPECT_EQ(ObjectFetchContext::FromDiskCache, request.origin);
}

TEST_F(ObjectStoreTest, getBlobSize_tracks_backing_store_read) {
  objectStore->getBlobSize(readyBlobId, context).get(0ms);
  ASSERT_EQ(1, loggingContext->requests.size());
  auto& request = loggingContext->requests[0];
  EXPECT_EQ(ObjectFetchContext::BlobMetadata, request.type);
  EXPECT_EQ(readyBlobId, request.hash);
  EXPECT_EQ(ObjectFetchContext::FromNetworkFetch, request.origin);
}

TEST_F(ObjectStoreTest, getBlobSize_tracks_second_read_from_cache) {
  objectStore->getBlobSize(readyBlobId, context).get(0ms);
  objectStore->getBlobSize(readyBlobId, context).get(0ms);
  ASSERT_EQ(2, loggingContext->requests.size());
  auto& request = loggingContext->requests[1];
  EXPECT_EQ(ObjectFetchContext::BlobMetadata, request.type);
  EXPECT_EQ(readyBlobId, request.hash);
  EXPECT_EQ(ObjectFetchContext::FromMemoryCache, request.origin);
}

TEST_F(ObjectStoreTest, getBlobSizeFromLocalStore) {
  auto data = "A"_sp;
  ObjectId id = putReadyBlob(data);

  // Get blob size from backing store, caches in local store
  objectStore->getBlobSize(id, context);
  // Clear backing store
  objectStore = ObjectStore::create(
      localStore,
      backingStore,
      treeCache,
      stats,
      std::make_shared<ProcessNameCache>(),
      std::make_shared<NullStructuredLogger>(),
      EdenConfig::createTestEdenConfig(),
      kPathMapDefaultCaseSensitive);

  size_t expectedSize = data.size();
  size_t size = objectStore->getBlobSize(id, context).get();
  EXPECT_EQ(expectedSize, size);
}

TEST_F(ObjectStoreTest, getBlobSizeFromBackingStore) {
  auto data = "A"_sp;
  ObjectId id = putReadyBlob(data);

  size_t expectedSize = data.size();
  size_t size = objectStore->getBlobSize(id, context).get();
  EXPECT_EQ(expectedSize, size);
}

TEST_F(ObjectStoreTest, getBlobSizeNotFound) {
  ObjectId id;

  EXPECT_THROW_RE(
      objectStore->getBlobSize(id, context).get(),
      std::domain_error,
      "blob .* not found");
}

TEST_F(ObjectStoreTest, getBlobSha1) {
  auto data = "A"_sp;
  ObjectId id = putReadyBlob(data);

  Hash20 expectedSha1 = Hash20::sha1(data);
  Hash20 sha1 = objectStore->getBlobSha1(id, context).get();
  EXPECT_EQ(expectedSha1.toString(), sha1.toString());
}

TEST_F(ObjectStoreTest, getBlobSha1NotFound) {
  ObjectId id;

  EXPECT_THROW_RE(
      objectStore->getBlobSha1(id, context).get(),
      std::domain_error,
      "blob .* not found");
}

TEST_F(ObjectStoreTest, get_size_and_sha1_only_imports_blob_once) {
  objectStore->getBlobSize(readyBlobId, context).get(0ms);
  objectStore->getBlobSha1(readyBlobId, context).get(0ms);

  EXPECT_EQ(1, fakeBackingStore->getAccessCount(readyBlobId));
}

class PidFetchContext final : public ObjectFetchContext {
 public:
  PidFetchContext(pid_t pid) : ObjectFetchContext{}, pid_{pid} {}

  std::optional<pid_t> getClientPid() const override {
    return pid_;
  }

  Cause getCause() const override {
    return Cause::Unknown;
  }

  const std::unordered_map<std::string, std::string>* FOLLY_NULLABLE
  getRequestInfo() const override {
    return nullptr;
  }

 private:
  pid_t pid_;
};

TEST_F(ObjectStoreTest, test_process_access_counts) {
  pid_t pid0{10000};
  ObjectFetchContextPtr pidContext0 = makeRefPtr<PidFetchContext>(pid0);
  pid_t pid1{10001};
  ObjectFetchContextPtr pidContext1 = makeRefPtr<PidFetchContext>(pid1);

  // first fetch increments fetch count for pid0
  objectStore->getBlob(readyBlobId, pidContext0).get(0ms);
  EXPECT_EQ(1, objectStore->getPidFetches().rlock()->at(pid0));

  // local fetch also increments fetch count for pid0
  objectStore->getBlob(readyBlobId, pidContext0).get(0ms);
  EXPECT_EQ(2, objectStore->getPidFetches().rlock()->at(pid0));

  // increments fetch count for pid1
  objectStore->getBlob(readyBlobId, pidContext1).get(0ms);
  EXPECT_EQ(2, objectStore->getPidFetches().rlock()->at(pid0));
  EXPECT_EQ(1, objectStore->getPidFetches().rlock()->at(pid1));
}

class FetchContext final : public ObjectFetchContext {
 public:
  FetchContext() = default;

  Cause getCause() const override {
    return Cause::Unknown;
  }

  const std::unordered_map<std::string, std::string>* FOLLY_NULLABLE
  getRequestInfo() const override {
    return nullptr;
  }

  void didFetch(ObjectType, const ObjectId&, Origin) override {
    ++fetchCount_;
  }

  uint64_t getFetchCount() const {
    return fetchCount_;
  }

 private:
  std::atomic<uint64_t> fetchCount_{0};
};

TEST_F(ObjectStoreTest, blobs_with_same_objectid_are_equal) {
  auto context = makeRefPtr<FetchContext>();

  auto objectId = putReadyBlob("foo");

  auto fut = objectStore->areBlobsEqual(
      objectId, objectId, context.as<ObjectFetchContext>());
  EXPECT_TRUE(std::move(fut).get(0ms));
  EXPECT_EQ(context->getFetchCount(), 0);
}

TEST_F(ObjectStoreTest, different_blobs_arent_equal) {
  auto context = makeRefPtr<FetchContext>();

  auto one = putReadyBlob("foo");
  auto two = putReadyBlob("bar");

  auto fut =
      objectStore->areBlobsEqual(one, two, context.as<ObjectFetchContext>());
  EXPECT_FALSE(std::move(fut).get(0ms));
  EXPECT_EQ(context->getFetchCount(), 2);
}

TEST_F(
    ObjectStoreTest,
    blobs_with_different_objectid_but_same_content_are_equal) {
  auto context = makeRefPtr<FetchContext>();

  auto one = putReadyBlob("foo");
  auto storedBlob =
      fakeBackingStore->putBlob(ObjectId{"not_a_content_hash"}, "foo");
  storedBlob->setReady();
  auto two = storedBlob->get().getHash();

  auto fut =
      objectStore->areBlobsEqual(one, two, context.as<ObjectFetchContext>());
  EXPECT_TRUE(std::move(fut).get(0ms));
  EXPECT_EQ(context->getFetchCount(), 2);
}
