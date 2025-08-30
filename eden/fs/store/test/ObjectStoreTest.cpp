/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/test/TestUtils.h>
#include <gtest/gtest.h>

#include "eden/common/telemetry/NullStructuredLogger.h"
#include "eden/common/utils/ImmediateFuture.h"
#include "eden/common/utils/ProcessInfoCache.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/model/TestOps.h"
#include "eden/fs/store/MemoryLocalStore.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/store/TreeCache.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/testharness/FakeBackingStore.h"
#include "eden/fs/testharness/LoggingFetchContext.h"
#include "eden/fs/testharness/StoredObject.h"

using namespace folly::string_piece_literals;
using namespace std::chrono_literals;

namespace facebook::eden {
namespace {

constexpr size_t kTreeCacheMaximumSize = 1000; // bytes
constexpr size_t kTreeCacheMinimumEntries = 0;
constexpr folly::StringPiece kBlake3Key = "19700101-1111111111111111111111#";

struct ObjectStoreTest : public ::testing::TestWithParam<CaseSensitivity> {
  void SetUp() override {
    std::shared_ptr<EdenConfig> rawEdenConfig{
        EdenConfig::createTestEdenConfig()};
    rawEdenConfig->inMemoryTreeCacheSize.setValue(
        kTreeCacheMaximumSize, ConfigSourceType::Default, true);
    rawEdenConfig->inMemoryTreeCacheMinimumItems.setValue(
        kTreeCacheMinimumEntries, ConfigSourceType::Default, true);
    auto edenConfig = std::make_shared<ReloadableConfig>(
        rawEdenConfig, ConfigReloadBehavior::NoReload);
    stats = makeRefPtr<EdenStats>();
    treeCache = TreeCache::create(edenConfig, stats.copy());
    localStore = std::make_shared<MemoryLocalStore>(stats.copy());
    fakeBackingStore = std::make_shared<FakeBackingStore>(
        BackingStore::LocalStoreCachingPolicy::Anything);
    objectStore = ObjectStore::create(
        fakeBackingStore,
        localStore,
        treeCache,
        stats.copy(),
        std::make_shared<ProcessInfoCache>(),
        std::make_shared<NullStructuredLogger>(),
        std::make_shared<ReloadableConfig>(
            EdenConfig::createTestEdenConfig(), ConfigReloadBehavior::NoReload),
        true,
        GetParam());

    auto configWithBlake3Key = EdenConfig::createTestEdenConfig();
    configWithBlake3Key->blake3Key.setStringValue(
        kBlake3Key, ConfigVariables(), ConfigSourceType::UserConfig);
    fakeBackingStoreWithKeyedBlake3 = std::make_shared<FakeBackingStore>(
        BackingStore::LocalStoreCachingPolicy::Anything,
        nullptr,
        std::string(kBlake3Key));
    objectStoreWithBlake3Key = ObjectStore::create(
        fakeBackingStoreWithKeyedBlake3,
        localStore,
        treeCache,
        stats.copy(),
        std::make_shared<ProcessInfoCache>(),
        std::make_shared<NullStructuredLogger>(),
        std::make_shared<ReloadableConfig>(
            configWithBlake3Key, ConfigReloadBehavior::NoReload),
        true,
        GetParam());

    auto configWithTreeAuxPrefetching = EdenConfig::createTestEdenConfig();
    ;
    configWithTreeAuxPrefetching->warmTreeAuxCacheIfTreeFromLocalStore.setValue(
        true, ConfigSourceType::UserConfig);
    fakeBackingStoreWithTreeAuxPrefetching = std::make_shared<FakeBackingStore>(
        BackingStore::LocalStoreCachingPolicy::Anything);
    objectStoreWithTreeAuxPrefetching = ObjectStore::create(
        fakeBackingStoreWithTreeAuxPrefetching,
        localStore,
        treeCache,
        stats.copy(),
        std::make_shared<ProcessInfoCache>(),
        std::make_shared<NullStructuredLogger>(),
        std::make_shared<ReloadableConfig>(
            configWithTreeAuxPrefetching, ConfigReloadBehavior::NoReload),
        true,
        GetParam());

    readyBlobId = putReadyBlob("readyblob");
    readyTreeId = putReadyTree();
  }

  ObjectId putReadyBlob(folly::StringPiece data) {
    {
      auto [storedBlob, id] = fakeBackingStoreWithKeyedBlake3->putBlob(data);
      storedBlob->setReady();
    }

    {
      auto [storedBlob, id] = fakeBackingStore->putBlob(data);
      storedBlob->setReady();
    }

    auto [storedBlob, id] =
        fakeBackingStoreWithTreeAuxPrefetching->putBlob(data);
    storedBlob->setReady();
    return id;
  }

  ObjectId putReadyTree() {
    {
      auto* storedBlob = fakeBackingStoreWithKeyedBlake3->putTree({});
      storedBlob->setReady();
    }

    {
      StoredTree* storedTree = fakeBackingStore->putTree({});
      storedTree->setReady();
    }

    StoredTree* storedTree =
        fakeBackingStoreWithTreeAuxPrefetching->putTree({});
    storedTree->setReady();

    return storedTree->get().getObjectId();
  }

  ObjectId putReadyTree(const Tree::container& entries) {
    {
      auto* storedBlob = fakeBackingStoreWithKeyedBlake3->putTree(entries);
      storedBlob->setReady();
    }

    {
      StoredTree* storedTree = fakeBackingStore->putTree(entries);
      storedTree->setReady();
    }

    StoredTree* storedTree =
        fakeBackingStoreWithTreeAuxPrefetching->putTree(entries);
    storedTree->setReady();
    return storedTree->get().getObjectId();
  }

  void putReadyGlob(
      std::pair<RootId, std::string> suffixQuery,
      std::vector<std::string> globPtr) {
    StoredGlob* storedGlob =
        fakeBackingStore->putGlob(suffixQuery, std::move(globPtr));
    storedGlob->setReady();
  }

  CaseSensitivity getOppositeCaseSensitivity() const {
    return GetParam() == CaseSensitivity::Sensitive
        ? CaseSensitivity::Insensitive
        : CaseSensitivity::Sensitive;
  }

  RefPtr<LoggingFetchContext> loggingContext =
      makeRefPtr<LoggingFetchContext>();
  const ObjectFetchContextPtr& context =
      loggingContext.as<ObjectFetchContext>();
  std::shared_ptr<LocalStore> localStore;
  std::shared_ptr<FakeBackingStore> fakeBackingStore;
  std::shared_ptr<FakeBackingStore> fakeBackingStoreWithKeyedBlake3;
  std::shared_ptr<FakeBackingStore> fakeBackingStoreWithTreeAuxPrefetching;
  std::shared_ptr<BackingStore> backingStoreWithKeyedBlake3;
  std::shared_ptr<TreeCache> treeCache;
  EdenStatsPtr stats;
  std::shared_ptr<ObjectStore> objectStore;
  std::shared_ptr<ObjectStore> objectStoreWithBlake3Key;
  std::shared_ptr<ObjectStore> objectStoreWithTreeAuxPrefetching;

  ObjectId readyBlobId;
  ObjectId readyTreeId;
};

} // namespace
TEST_P(ObjectStoreTest, getBlob_tracks_backing_store_read) {
  objectStore->getBlob(readyBlobId, context).get(0ms);
  ASSERT_EQ(1, loggingContext->requests.size());
  auto& request = loggingContext->requests[0];
  EXPECT_EQ(ObjectFetchContext::Blob, request.type);
  EXPECT_EQ(readyBlobId, request.id);
  EXPECT_EQ(ObjectFetchContext::FromNetworkFetch, request.origin);
}

TEST_P(ObjectStoreTest, caching_policies_anything) {
  objectStore->setLocalStoreCachingPolicy(
      BackingStore::LocalStoreCachingPolicy::Anything);
  EXPECT_TRUE(objectStore->shouldCacheOnDisk(
      BackingStore::LocalStoreCachingPolicy::Trees));
  EXPECT_TRUE(objectStore->shouldCacheOnDisk(
      BackingStore::LocalStoreCachingPolicy::Blobs));
  EXPECT_TRUE(objectStore->shouldCacheOnDisk(
      BackingStore::LocalStoreCachingPolicy::BlobAuxData));
  EXPECT_TRUE(objectStore->shouldCacheOnDisk(
      BackingStore::LocalStoreCachingPolicy::TreesAndBlobAuxData));
  EXPECT_TRUE(objectStore->shouldCacheOnDisk(
      BackingStore::LocalStoreCachingPolicy::Anything));
  EXPECT_FALSE(objectStore->shouldCacheOnDisk(
      BackingStore::LocalStoreCachingPolicy::NoCaching));
}

TEST_P(ObjectStoreTest, caching_policies_no_caching) {
  objectStore->setLocalStoreCachingPolicy(
      BackingStore::LocalStoreCachingPolicy::NoCaching);
  EXPECT_FALSE(objectStore->shouldCacheOnDisk(
      BackingStore::LocalStoreCachingPolicy::Trees));
  EXPECT_FALSE(objectStore->shouldCacheOnDisk(
      BackingStore::LocalStoreCachingPolicy::Blobs));
  EXPECT_FALSE(objectStore->shouldCacheOnDisk(
      BackingStore::LocalStoreCachingPolicy::BlobAuxData));
  EXPECT_FALSE(objectStore->shouldCacheOnDisk(
      BackingStore::LocalStoreCachingPolicy::TreesAndBlobAuxData));
  EXPECT_FALSE(objectStore->shouldCacheOnDisk(
      BackingStore::LocalStoreCachingPolicy::Anything));
  EXPECT_FALSE(objectStore->shouldCacheOnDisk(
      BackingStore::LocalStoreCachingPolicy::NoCaching));
}
TEST_P(ObjectStoreTest, caching_policies_blob) {
  objectStore->setLocalStoreCachingPolicy(
      BackingStore::LocalStoreCachingPolicy::Blobs);
  EXPECT_FALSE(objectStore->shouldCacheOnDisk(
      BackingStore::LocalStoreCachingPolicy::Trees));
  EXPECT_TRUE(objectStore->shouldCacheOnDisk(
      BackingStore::LocalStoreCachingPolicy::Blobs));
  EXPECT_FALSE(objectStore->shouldCacheOnDisk(
      BackingStore::LocalStoreCachingPolicy::BlobAuxData));
  EXPECT_FALSE(objectStore->shouldCacheOnDisk(
      BackingStore::LocalStoreCachingPolicy::TreesAndBlobAuxData));
  EXPECT_TRUE(objectStore->shouldCacheOnDisk(
      BackingStore::LocalStoreCachingPolicy::Anything));
  EXPECT_FALSE(objectStore->shouldCacheOnDisk(
      BackingStore::LocalStoreCachingPolicy::NoCaching));
}

TEST_P(ObjectStoreTest, caching_policies_trees) {
  objectStore->setLocalStoreCachingPolicy(
      BackingStore::LocalStoreCachingPolicy::Trees);
  EXPECT_TRUE(objectStore->shouldCacheOnDisk(
      BackingStore::LocalStoreCachingPolicy::Trees));
  EXPECT_FALSE(objectStore->shouldCacheOnDisk(
      BackingStore::LocalStoreCachingPolicy::Blobs));
  EXPECT_FALSE(objectStore->shouldCacheOnDisk(
      BackingStore::LocalStoreCachingPolicy::BlobAuxData));
  EXPECT_TRUE(objectStore->shouldCacheOnDisk(
      BackingStore::LocalStoreCachingPolicy::TreesAndBlobAuxData));
  EXPECT_TRUE(objectStore->shouldCacheOnDisk(
      BackingStore::LocalStoreCachingPolicy::Anything));
  EXPECT_FALSE(objectStore->shouldCacheOnDisk(
      BackingStore::LocalStoreCachingPolicy::NoCaching));
}

TEST_P(ObjectStoreTest, caching_policies_blob_aux_data) {
  objectStore->setLocalStoreCachingPolicy(
      BackingStore::LocalStoreCachingPolicy::BlobAuxData);
  EXPECT_FALSE(objectStore->shouldCacheOnDisk(
      BackingStore::LocalStoreCachingPolicy::Trees));
  EXPECT_FALSE(objectStore->shouldCacheOnDisk(
      BackingStore::LocalStoreCachingPolicy::Blobs));
  EXPECT_TRUE(objectStore->shouldCacheOnDisk(
      BackingStore::LocalStoreCachingPolicy::BlobAuxData));
  EXPECT_TRUE(objectStore->shouldCacheOnDisk(
      BackingStore::LocalStoreCachingPolicy::TreesAndBlobAuxData));
  EXPECT_TRUE(objectStore->shouldCacheOnDisk(
      BackingStore::LocalStoreCachingPolicy::Anything));
  EXPECT_FALSE(objectStore->shouldCacheOnDisk(
      BackingStore::LocalStoreCachingPolicy::NoCaching));
}

TEST_P(ObjectStoreTest, caching_policies_trees_and_blob_aux_data) {
  objectStore->setLocalStoreCachingPolicy(
      BackingStore::LocalStoreCachingPolicy::TreesAndBlobAuxData);
  EXPECT_TRUE(objectStore->shouldCacheOnDisk(
      BackingStore::LocalStoreCachingPolicy::Trees));
  EXPECT_FALSE(objectStore->shouldCacheOnDisk(
      BackingStore::LocalStoreCachingPolicy::Blobs));
  EXPECT_TRUE(objectStore->shouldCacheOnDisk(
      BackingStore::LocalStoreCachingPolicy::BlobAuxData));
  EXPECT_TRUE(objectStore->shouldCacheOnDisk(
      BackingStore::LocalStoreCachingPolicy::TreesAndBlobAuxData));
  EXPECT_TRUE(objectStore->shouldCacheOnDisk(
      BackingStore::LocalStoreCachingPolicy::Anything));
  EXPECT_FALSE(objectStore->shouldCacheOnDisk(
      BackingStore::LocalStoreCachingPolicy::NoCaching));
}
TEST_P(ObjectStoreTest, getBlob_tracks_second_read_from_cache) {
  objectStore->getBlob(readyBlobId, context).get(0ms);
  objectStore->getBlob(readyBlobId, context).get(0ms);
  ASSERT_EQ(2, loggingContext->requests.size());
  auto& request = loggingContext->requests[1];
  EXPECT_EQ(ObjectFetchContext::Blob, request.type);
  EXPECT_EQ(readyBlobId, request.id);
  EXPECT_EQ(ObjectFetchContext::FromDiskCache, request.origin);
}

TEST_P(ObjectStoreTest, getTree_tracks_backing_store_read) {
  objectStore->getTree(readyTreeId, context).get(0ms);
  ASSERT_EQ(1, loggingContext->requests.size());
  auto& request = loggingContext->requests[0];
  EXPECT_EQ(ObjectFetchContext::Tree, request.type);
  EXPECT_EQ(readyTreeId, request.id);
  EXPECT_EQ(ObjectFetchContext::FromNetworkFetch, request.origin);
}

TEST_P(ObjectStoreTest, getTree_tracks_second_read_from_cache) {
  objectStore->getTree(readyTreeId, context).get(0ms);
  objectStore->getTree(readyTreeId, context).get(0ms);
  ASSERT_EQ(2, loggingContext->requests.size());
  auto& request = loggingContext->requests[1];
  EXPECT_EQ(ObjectFetchContext::Tree, request.type);
  EXPECT_EQ(readyTreeId, request.id);
  EXPECT_EQ(ObjectFetchContext::FromMemoryCache, request.origin);
}

TEST_P(ObjectStoreTest, getTree_tracks_second_read_from_local_store) {
  objectStore->getTree(readyTreeId, context).get(0ms);

  // clear the in memory cache so the tree can not be found here
  treeCache->clear();

  objectStore->getTree(readyTreeId, context).get(0ms);
  ASSERT_EQ(2, loggingContext->requests.size());
  auto& request = loggingContext->requests[1];
  EXPECT_EQ(ObjectFetchContext::Tree, request.type);
  EXPECT_EQ(readyTreeId, request.id);
  EXPECT_EQ(ObjectFetchContext::FromDiskCache, request.origin);
}

TEST_P(ObjectStoreTest, getTree_prefetch_missing_aux_data) {
  // FakeBackingStore provides a tree with no aux data; this simulates storing a
  // tree that lacks aux data in LocalStore
  auto tree1 =
      objectStoreWithTreeAuxPrefetching->getTree(readyTreeId, context).get(0ms);
  EXPECT_EQ(tree1->getAuxData(), nullptr);

  // Clear in-memory cache so that we are guaranteed to hit LocalStore caches
  treeCache->clear();

  // Issue a getTree that triggers tree aux prefetching. Since tree aux is
  // missing from the LocalStore tree, we will attempt to fall back to
  // BackingStore->getTreeAux() which will also fail.
  auto tree2 =
      objectStoreWithTreeAuxPrefetching->getTree(readyTreeId, context).get(0ms);

  EXPECT_EQ(tree2->getAuxData(), nullptr);
}

TEST_P(ObjectStoreTest, getBlobSize_tracks_backing_store_read) {
  objectStore->getBlobSize(readyBlobId, context).get(0ms);
  ASSERT_EQ(1, loggingContext->requests.size());
  auto& request = loggingContext->requests[0];
  EXPECT_EQ(ObjectFetchContext::BlobAuxData, request.type);
  EXPECT_EQ(readyBlobId, request.id);
  EXPECT_EQ(ObjectFetchContext::FromNetworkFetch, request.origin);
}

TEST_P(ObjectStoreTest, getBlobSize_tracks_second_read_from_cache) {
  objectStore->getBlobSize(readyBlobId, context).get(0ms);
  objectStore->getBlobSize(readyBlobId, context).get(0ms);
  ASSERT_EQ(2, loggingContext->requests.size());
  auto& request = loggingContext->requests[1];
  EXPECT_EQ(ObjectFetchContext::BlobAuxData, request.type);
  EXPECT_EQ(readyBlobId, request.id);
  EXPECT_EQ(ObjectFetchContext::FromMemoryCache, request.origin);
}

TEST_P(ObjectStoreTest, getBlobSizeFromLocalStore) {
  auto data = "A"_sp;
  ObjectId id = putReadyBlob(data);

  // Get blob size from backing store, caches in local store
  objectStore->getBlobSize(id, context);
  // Clear backing store
  objectStore = ObjectStore::create(
      fakeBackingStore,
      localStore,
      treeCache,
      stats.copy(),
      std::make_shared<ProcessInfoCache>(),
      std::make_shared<NullStructuredLogger>(),
      std::make_shared<ReloadableConfig>(
          EdenConfig::createTestEdenConfig(), ConfigReloadBehavior::NoReload),
      true,
      GetParam());

  size_t expectedSize = data.size();
  size_t size = objectStore->getBlobSize(id, context).get();
  EXPECT_EQ(expectedSize, size);
}

TEST_P(ObjectStoreTest, getBlobSizeFromBackingStore) {
  auto data = "A"_sp;
  ObjectId id = putReadyBlob(data);

  size_t expectedSize = data.size();
  size_t size = objectStore->getBlobSize(id, context).get();
  EXPECT_EQ(expectedSize, size);
}

TEST_P(ObjectStoreTest, getBlobSizeNotFound) {
  ObjectId id;

  EXPECT_THROW_RE(
      objectStore->getBlobSize(id, context).get(),
      std::domain_error,
      "blob .* not found");
}

TEST_P(ObjectStoreTest, getBlobSha1) {
  auto data = "A"_sp;
  ObjectId id = putReadyBlob(data);

  Hash20 expectedSha1 = Hash20::sha1(data);
  Hash20 sha1 = objectStore->getBlobSha1(id, context).get();
  EXPECT_EQ(expectedSha1.toString(), sha1.toString());
}

TEST_P(ObjectStoreTest, getBlobBlake3) {
  auto data = "A"_sp;
  ObjectId id = putReadyBlob(data);

  Hash32 expectedBlake3 = Hash32::blake3(data);
  Hash32 blake3 = objectStore->getBlobBlake3(id, context).get();
  EXPECT_EQ(expectedBlake3.toString(), blake3.toString());
}

TEST_P(ObjectStoreTest, getBlobBlake3IsMissingInLocalStore) {
  auto data = "A"_sp;
  ObjectId id = putReadyBlob(data);
  BlobAuxData blobAuxdata(Hash20::sha1(data), std::nullopt, data.size());
  localStore->putBlobAuxData(id, blobAuxdata);

  const auto blake3Try =
      objectStoreWithBlake3Key->getBlobBlake3(id, context).getTry();
  ASSERT_TRUE(blake3Try.hasValue());
  Hash32 expectedBlake3 =
      Hash32::keyedBlake3(folly::ByteRange{kBlake3Key}, data);
  EXPECT_EQ(blake3Try->toString(), expectedBlake3.toString());
}

TEST_P(ObjectStoreTest, getBlobKeyedBlake3) {
  auto data = "A"_sp;
  ObjectId id = putReadyBlob(data);

  Hash32 expectedBlake3 =
      Hash32::keyedBlake3(folly::ByteRange{kBlake3Key}, data);
  Hash32 blake3 = objectStoreWithBlake3Key->getBlobBlake3(id, context).get();
  EXPECT_EQ(expectedBlake3.toString(), blake3.toString());
}

TEST_P(ObjectStoreTest, getBlobSha1NotFound) {
  ObjectId id;

  EXPECT_THROW_RE(
      objectStore->getBlobSha1(id, context).get(),
      std::domain_error,
      "blob .* not found");
}

TEST_P(ObjectStoreTest, getBlobBlake3NotFound) {
  ObjectId id;

  EXPECT_THROW_RE(
      objectStore->getBlobBlake3(id, context).get(),
      std::domain_error,
      "blob .* not found");
}

TEST_P(ObjectStoreTest, get_size_and_sha1_and_blake3_only_imports_blob_once) {
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

TEST_P(ObjectStoreTest, test_process_access_counts) {
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

  void didFetch(ObjectType, const ObjectId&, Origin origin) override {
    ++fetchCount_;
    origin_ = origin;
  }

  uint64_t getFetchCount() const {
    return fetchCount_;
  }

  Origin getFetchedOrigin() const {
    return origin_;
  }

 private:
  std::atomic<uint64_t> fetchCount_{0};
  std::atomic<Origin> origin_{Origin::NotFetched};
};

TEST_P(ObjectStoreTest, blobs_with_same_objectid_are_equal) {
  auto context = makeRefPtr<FetchContext>();

  auto objectId = putReadyBlob("foo");

  auto fut = objectStore->areBlobsEqual(
      objectId, objectId, context.as<ObjectFetchContext>());
  EXPECT_TRUE(std::move(fut).get(0ms));
  EXPECT_EQ(context->getFetchCount(), 0);
}

TEST_P(ObjectStoreTest, different_blobs_arent_equal) {
  auto context = makeRefPtr<FetchContext>();

  auto one = putReadyBlob("foo");
  auto two = putReadyBlob("bar");

  auto fut =
      objectStore->areBlobsEqual(one, two, context.as<ObjectFetchContext>());
  EXPECT_FALSE(std::move(fut).get(0ms));
  EXPECT_EQ(context->getFetchCount(), 2);
}

TEST_P(
    ObjectStoreTest,
    blobs_with_different_objectid_but_same_content_are_equal) {
  auto context = makeRefPtr<FetchContext>();

  auto one = putReadyBlob("foo");
  auto two = ObjectId{"not_a_constant_id"};
  auto storedBlob = fakeBackingStore->putBlob(two, "foo");
  storedBlob->setReady();

  auto fut =
      objectStore->areBlobsEqual(one, two, context.as<ObjectFetchContext>());
  EXPECT_TRUE(std::move(fut).get(0ms));
  EXPECT_EQ(context->getFetchCount(), 2);
}

TEST_P(ObjectStoreTest, glob_files_test) {
  RootId rootId{"00000000000000000000"};
  auto glob = std::vector<std::string>{"foo.txt", "bar.txt"};
  putReadyGlob(std::pair<RootId, std::string>(rootId, ".txt"), std::move(glob));

  auto context = makeRefPtr<FetchContext>();
  auto globs = std::vector<std::string>{".txt"};

  auto fut = objectStore->getGlobFiles(
      rootId,
      globs,
      std::vector<std::string>{},
      context.as<ObjectFetchContext>());
  auto result = std::move(fut).get(0ms);
  EXPECT_EQ(result.globFiles.size(), 2);
  auto sorted_result = result.globFiles;
  std::sort(sorted_result.begin(), sorted_result.end());
  auto expected_result = std::vector<std::string>{"bar.txt", "foo.txt"};
  for (int i = 0; i < 2; i++) {
    EXPECT_EQ(sorted_result[i], expected_result[i]);
  }
}

TEST_P(ObjectStoreTest, get_tree_with_different_sensitivities) {
  auto context1 = makeRefPtr<FetchContext>();
  auto context2 = makeRefPtr<FetchContext>();
  auto context3 = makeRefPtr<FetchContext>();
  auto context4 = makeRefPtr<FetchContext>();

  // construct an object store with the opposite case sensitivity that shares
  // the same treecache
  auto oppositeSensitivityObjectStore = ObjectStore::create(
      fakeBackingStore,
      localStore,
      treeCache,
      stats.copy(),
      std::make_shared<ProcessInfoCache>(),
      std::make_shared<NullStructuredLogger>(),
      std::make_shared<ReloadableConfig>(
          EdenConfig::createTestEdenConfig(), ConfigReloadBehavior::NoReload),
      true,
      getOppositeCaseSensitivity());

  auto blobOne = putReadyBlob("foo content");
  auto blobTwo = putReadyBlob("bar content");

  // put a tree with the correct case sensitivity
  auto treeId = putReadyTree(
      {{{PathComponent{"Bar"},
         TreeEntry{blobOne, TreeEntryType::EXECUTABLE_FILE}}},
       GetParam()});

  // put a tree with the opposite case sensitivity
  auto oppositeSensitivityTreeId = putReadyTree(
      {{{PathComponent{"foo"}, TreeEntry{blobOne, TreeEntryType::REGULAR_FILE}},
        {PathComponent{"Baz"}, TreeEntry{readyTreeId, TreeEntryType::TREE}}},
       getOppositeCaseSensitivity()});

  // fetch the "GetParam() case sensitivity" tree from the "GetParam() case
  // sensitivity" object store. this should populate the treecache
  auto treeFromMatchingObjectStoreResult =
      objectStore->getTree(treeId, context1.as<ObjectFetchContext>()).get(0ms);

  EXPECT_EQ(context1->getFetchedOrigin(), ObjectFetchContext::FromNetworkFetch);
  EXPECT_EQ(
      treeFromMatchingObjectStoreResult->getCaseSensitivity(), GetParam());

  // fetch the "getOppositeCaseSensitivity() case sensitivity" tree from the
  // "GetParam() case sensitivity" object store. this should populate the
  // treecache
  auto oppositeSensitivityTreeFromMatchingObjectStoreResult =
      objectStore
          ->getTree(
              oppositeSensitivityTreeId, context2.as<ObjectFetchContext>())
          .get(0ms);

  EXPECT_EQ(context2->getFetchedOrigin(), ObjectFetchContext::FromNetworkFetch);
  EXPECT_EQ(
      oppositeSensitivityTreeFromMatchingObjectStoreResult
          ->getCaseSensitivity(),
      GetParam());

  // get the "GetParam() case sensitivity" from the
  // "getOppositeCaseSensitivity() case sensitivity" object store. this should
  // not fetch from the backing store and should instead be served from the
  // treecache populated by the first object store's call to getTree
  auto treeFromOppositeObjectStoreResult =
      oppositeSensitivityObjectStore
          ->getTree(treeId, context3.as<ObjectFetchContext>())
          .get(0ms);

  EXPECT_EQ(context3->getFetchedOrigin(), ObjectFetchContext::FromMemoryCache);
  EXPECT_EQ(
      treeFromOppositeObjectStoreResult->getCaseSensitivity(),
      getOppositeCaseSensitivity());

  // get the "getOppositeCaseSensitivity() case sensitivity" from the
  // "getOppositeCaseSensitivity() case sensitivity" object store. this should
  // not fetch from the backing store and should instead be served from the
  // treecache populated by the first object store's call to getTree
  auto oppositeSensitivityTreeFromOppositeObjectStoreResult =
      oppositeSensitivityObjectStore
          ->getTree(
              oppositeSensitivityTreeId, context4.as<ObjectFetchContext>())
          .get(0ms);

  EXPECT_EQ(context4->getFetchedOrigin(), ObjectFetchContext::FromMemoryCache);
  EXPECT_EQ(
      oppositeSensitivityTreeFromOppositeObjectStoreResult
          ->getCaseSensitivity(),
      getOppositeCaseSensitivity());
}

INSTANTIATE_TEST_SUITE_P(
    ObjectStoreTest,
    ObjectStoreTest,
    ::testing::Values(
        CaseSensitivity::Sensitive,
        CaseSensitivity::Insensitive));

} // namespace facebook::eden
