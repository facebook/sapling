/*
 *  Copyright (c) 2018-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/store/BlobAccess.h"
#include <gtest/gtest.h>
#include <chrono>
#include "eden/fs/store/BlobCache.h"
#include "eden/fs/testharness/FakeObjectStore.h"

using namespace folly::literals;
using namespace std::chrono_literals;
using namespace facebook::eden;

namespace {
const auto hash3 = Hash{"0000000000000000000000000000000000000000"_sp};
const auto hash4 = Hash{"0000000000000000000000000000000000000001"_sp};
const auto hash5 = Hash{"0000000000000000000000000000000000000002"_sp};
const auto hash6 = Hash{"0000000000000000000000000000000000000003"_sp};

const auto blob3 = std::make_shared<Blob>(hash3, "333"_sp);
const auto blob4 = std::make_shared<Blob>(hash4, "4444"_sp);
const auto blob5 = std::make_shared<Blob>(hash5, "55555"_sp);
const auto blob6 = std::make_shared<Blob>(hash6, "666666"_sp);
} // namespace

struct BlobAccessTest : ::testing::Test {
  BlobAccessTest()
      : objectStore{std::make_shared<FakeObjectStore>()},
        blobCache{BlobCache::create(10, 0)},
        blobAccess{objectStore, blobCache} {
    objectStore->addBlob(Blob{hash3, "333"_sp});
    objectStore->addBlob(Blob{hash4, "4444"_sp});
    objectStore->addBlob(Blob{hash5, "55555"_sp});
    objectStore->addBlob(Blob{hash6, "666666"_sp});
  }
  std::shared_ptr<FakeObjectStore> objectStore;
  std::shared_ptr<BlobCache> blobCache;
  BlobAccess blobAccess;
};

TEST_F(BlobAccessTest, remembers_blobs) {
  auto blob1 = blobAccess.getBlob(hash4).get(0ms).blob;
  auto blob2 = blobAccess.getBlob(hash4).get(0ms).blob;

  EXPECT_EQ(blob1, blob2);
  EXPECT_EQ(4, blob1->getSize());
  EXPECT_EQ(1, objectStore->getAccessCount(hash4));
}

TEST_F(BlobAccessTest, drops_blobs_when_size_is_exceeded) {
  auto blob1 = blobAccess.getBlob(hash6).get(0ms).blob;
  auto blob2 = blobAccess.getBlob(hash5).get(0ms).blob;
  auto blob3 = blobAccess.getBlob(hash6).get(0ms).blob;

  EXPECT_EQ(6, blob1->getSize());
  EXPECT_EQ(5, blob2->getSize());
  EXPECT_EQ(6, blob3->getSize());

  EXPECT_EQ(1, objectStore->getAccessCount(hash5));
  EXPECT_EQ(2, objectStore->getAccessCount(hash6));
}

TEST_F(BlobAccessTest, drops_oldest_blobs) {
  blobAccess.getBlob(hash3).get(0ms);
  blobAccess.getBlob(hash4).get(0ms);

  // Evicts hash3
  blobAccess.getBlob(hash5).get(0ms);
  EXPECT_EQ(1, objectStore->getAccessCount(hash3));
  EXPECT_EQ(1, objectStore->getAccessCount(hash4));
  EXPECT_EQ(1, objectStore->getAccessCount(hash5));

  // Evicts hash4 but not hash5
  blobAccess.getBlob(hash3).get(0ms);
  blobAccess.getBlob(hash5).get(0ms);
  EXPECT_EQ(2, objectStore->getAccessCount(hash3));
  EXPECT_EQ(1, objectStore->getAccessCount(hash4));
  EXPECT_EQ(1, objectStore->getAccessCount(hash5));

  // Evicts hash3
  blobAccess.getBlob(hash4).get(0ms);
  blobAccess.getBlob(hash5).get(0ms);
  EXPECT_EQ(2, objectStore->getAccessCount(hash3));
  EXPECT_EQ(2, objectStore->getAccessCount(hash4));
  EXPECT_EQ(1, objectStore->getAccessCount(hash5));
}
