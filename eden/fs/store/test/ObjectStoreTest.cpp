/*
 *  Copyright (c) 2019-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
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

  std::shared_ptr<LocalStore> localStore_;
  std::shared_ptr<FakeBackingStore> backingStore_;
  std::shared_ptr<ObjectStore> objectStore_;
};

TEST_F(ObjectStoreTest, getBlobSize) {
  folly::StringPiece data = "A";

  StoredBlob* storedBlob = backingStore_->putBlob(data);
  storedBlob->setReady();

  Blob blob = storedBlob->get();
  Hash id = blob.getHash();

  size_t size = objectStore_->getSize(id).get();
  EXPECT_EQ(data.size(), size);
}

TEST_F(ObjectStoreTest, getBlobSizeNotFound) {
  Hash id;

  EXPECT_THROW_RE(
      objectStore_->getSize(id).get(), std::domain_error, "blob .* not found");
}
