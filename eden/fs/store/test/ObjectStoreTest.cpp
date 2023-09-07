/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/portability/GTest.h>
#include <folly/test/TestUtils.h>

#include "eden/common/utils/ProcessInfoCache.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/model/TestOps.h"
#include "eden/fs/store/LocalStoreCachedBackingStore.h"
#include "eden/fs/store/MemoryLocalStore.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/store/TreeCache.h"
#include "eden/fs/telemetry/EdenStats.h"
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
constexpr folly::StringPiece kBlake3Key = "19700101-1111111111111111111111#";

struct ObjectStoreTest : ::testing::Test {
  void SetUp() override {
    std::shared_ptr<EdenConfig> rawEdenConfig{
        EdenConfig::createTestEdenConfig()};
    rawEdenConfig->inMemoryTreeCacheSize.setValue(
        kTreeCacheMaximumSize, ConfigSourceType::Default, true);
    rawEdenConfig->inMemoryTreeCacheMinimumItems.setValue(
        kTreeCacheMinimumEntries, ConfigSourceType::Default, true);
    auto edenConfig = std::make_shared<ReloadableConfig>(
        rawEdenConfig, ConfigReloadBehavior::NoReload);
    treeCache = TreeCache::create(edenConfig);
    stats = makeRefPtr<EdenStats>();
    localStore = std::make_shared<MemoryLocalStore>(stats.copy());
    fakeBackingStore = std::make_shared<FakeBackingStore>();
    backingStore = std::make_shared<LocalStoreCachedBackingStore>(
        fakeBackingStore,
        localStore,
        stats.copy(),
        LocalStoreCachedBackingStore::CachingPolicy::Everything);
    objectStore = ObjectStore::create(
        backingStore,
        treeCache,
        stats.copy(),
        std::make_shared<ProcessInfoCache>(),
        std::make_shared<NullStructuredLogger>(),
        EdenConfig::createTestEdenConfig(),
        true,
        kPathMapDefaultCaseSensitive);

    auto configWithBlake3Key = EdenConfig::createTestEdenConfig();
    configWithBlake3Key->blake3Key.setStringValue(
        kBlake3Key, ConfigVariables(), ConfigSourceType::UserConfig);
    fakeBackingStoreWithKeyedBlake3 =
        std::make_shared<FakeBackingStore>(std::string(kBlake3Key));
    backingStoreWithKeyedBlake3 =
        std::make_shared<LocalStoreCachedBackingStore>(
            fakeBackingStoreWithKeyedBlake3,
            localStore,
            stats.copy(),
            LocalStoreCachedBackingStore::CachingPolicy::Everything);
    objectStoreWithBlake3Key = ObjectStore::create(
        backingStoreWithKeyedBlake3,
        treeCache,
        stats.copy(),
        std::make_shared<ProcessInfoCache>(),
        std::make_shared<NullStructuredLogger>(),
        std::move(configWithBlake3Key),
        true,
        kPathMapDefaultCaseSensitive);

    readyBlobId = putReadyBlob("readyblob");
    readyTreeId = putReadyTree();
  }

  ObjectId putReadyBlob(folly::StringPiece data) {
    {
      auto [storedBlob, id] = fakeBackingStoreWithKeyedBlake3->putBlob(data);
      storedBlob->setReady();
    }

    auto [storedBlob, id] = fakeBackingStore->putBlob(data);
    storedBlob->setReady();
    return id;
  }

  ObjectId putReadyTree() {
    {
      auto* storedBlob = fakeBackingStoreWithKeyedBlake3->putTree({});
      storedBlob->setReady();
    }

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
  std::shared_ptr<FakeBackingStore> fakeBackingStoreWithKeyedBlake3;
  std::shared_ptr<BackingStore> backingStoreWithKeyedBlake3;
  std::shared_ptr<TreeCache> treeCache;
  EdenStatsPtr stats;
  std::shared_ptr<ObjectStore> objectStore;
  std::shared_ptr<ObjectStore> objectStoreWithBlake3Key;

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
      backingStore,
      treeCache,
      stats.copy(),
      std::make_shared<ProcessInfoCache>(),
      std::make_shared<NullStructuredLogger>(),
      EdenConfig::createTestEdenConfig(),
      true,
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

TEST_F(ObjectStoreTest, getBlobBlake3) {
  auto data = "A"_sp;
  ObjectId id = putReadyBlob(data);

  Hash32 expectedBlake3 = Hash32::blake3(data);
  Hash32 blake3 = objectStore->getBlobBlake3(id, context).get();
  EXPECT_EQ(expectedBlake3.toString(), blake3.toString());
}

TEST_F(ObjectStoreTest, getBlobBlake3IsMissingInLocalStore) {
  auto data = "A"_sp;
  ObjectId id = putReadyBlob(data);
  BlobMetadata blobMetadata(Hash20::sha1(data), std::nullopt, data.size());
  localStore->putBlobMetadata(id, blobMetadata);

  const auto blake3Try =
      objectStoreWithBlake3Key->getBlobBlake3(id, context).getTry();
  ASSERT_TRUE(blake3Try.hasValue());
  Hash32 expectedBlake3 =
      Hash32::keyedBlake3(folly::ByteRange{kBlake3Key}, data);
  EXPECT_EQ(blake3Try->toString(), expectedBlake3.toString());
}

TEST_F(ObjectStoreTest, getBlobKeyedBlake3) {
  auto data = "A"_sp;
  ObjectId id = putReadyBlob(data);

  Hash32 expectedBlake3 =
      Hash32::keyedBlake3(folly::ByteRange{kBlake3Key}, data);
  Hash32 blake3 = objectStoreWithBlake3Key->getBlobBlake3(id, context).get();
  EXPECT_EQ(expectedBlake3.toString(), blake3.toString());
}

TEST_F(ObjectStoreTest, getBlobSha1NotFound) {
  ObjectId id;

  EXPECT_THROW_RE(
      objectStore->getBlobSha1(id, context).get(),
      std::domain_error,
      "blob .* not found");
}

TEST_F(ObjectStoreTest, getBlobBlake3NotFound) {
  ObjectId id;

  EXPECT_THROW_RE(
      objectStore->getBlobBlake3(id, context).get(),
      std::domain_error,
      "blob .* not found");
}

TEST_F(ObjectStoreTest, get_size_and_sha1_and_blake3_only_imports_blob_once) {
  objectStore->getBlobSize(readyBlobId, context).get(0ms);
  objectStore->getBlobSha1(readyBlobId, context).get(0ms);
  objectStore->getBlobBlake3(readyBlobId, context).get(0ms);

  EXPECT_EQ(1, fakeBackingStore->getAccessCount(readyBlobId));
}

class PidFetchContext final : public ObjectFetchContext {
 public:
  explicit PidFetchContext(ProcessId pid) : ObjectFetchContext{}, pid_{pid} {}

  OptionalProcessId getClientPid() const override {
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
  ProcessId pid_;
};

TEST_F(ObjectStoreTest, test_process_access_counts) {
  auto pid0 = ProcessId(10000);
  ObjectFetchContextPtr pidContext0 = makeRefPtr<PidFetchContext>(pid0);
  auto pid1 = ProcessId(10001);
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
  auto two = ObjectId{"not_a_constant_hash"};
  auto storedBlob = fakeBackingStore->putBlob(two, "foo");
  storedBlob->setReady();

  auto fut =
      objectStore->areBlobsEqual(one, two, context.as<ObjectFetchContext>());
  EXPECT_TRUE(std::move(fut).get(0ms));
  EXPECT_EQ(context->getFetchCount(), 2);
}
