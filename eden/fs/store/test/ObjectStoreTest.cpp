/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include <folly/test/TestUtils.h>
#include <gtest/gtest.h>

#include "eden/fs/store/MemoryLocalStore.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/testharness/FakeBackingStore.h"
#include "eden/fs/testharness/StoredObject.h"

using namespace facebook::eden;

class ObjectStoreTest : public ::testing::Test {
 protected:
  void SetUp() override {
    localStore_ = std::make_shared<MemoryLocalStore>();
    backingStore_ = std::make_shared<FakeBackingStore>(localStore_);
    objectStore_ = ObjectStore::create(localStore_, backingStore_);
  }

  Hash putReadyBlob(folly::StringPiece data) {
    StoredBlob* storedBlob = backingStore_->putBlob(data);
    storedBlob->setReady();

    Blob blob = storedBlob->get();
    return blob.getHash();
  }

  std::shared_ptr<LocalStore> localStore_;
  std::shared_ptr<FakeBackingStore> backingStore_;
  std::shared_ptr<ObjectStore> objectStore_;
};

TEST_F(ObjectStoreTest, getBlobSize) {
  folly::StringPiece data = "A";
  Hash id = putReadyBlob(data);

  size_t expectedSize = data.size();
  size_t size = objectStore_->getBlobSize(id).get();
  EXPECT_EQ(expectedSize, size);
}

TEST_F(ObjectStoreTest, getBlobSizeNotFound) {
  Hash id;

  EXPECT_THROW_RE(
      objectStore_->getBlobSize(id).get(),
      std::domain_error,
      "blob .* not found");
}

TEST_F(ObjectStoreTest, getBlobSha1) {
  folly::StringPiece data = "A";
  Hash id = putReadyBlob(data);

  Hash expectedSha1 = Hash::sha1(data);
  Hash sha1 = objectStore_->getBlobSha1(id).get();
  EXPECT_EQ(expectedSha1.toString(), sha1.toString());
}

TEST_F(ObjectStoreTest, getBlobSha1NotFound) {
  Hash id;

  EXPECT_THROW_RE(
      objectStore_->getBlobSha1(id).get(),
      std::domain_error,
      "blob .* not found");
}
