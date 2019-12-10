/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/testharness/FakeBackingStore.h"

#include <folly/experimental/TestUtil.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/test/TestUtils.h>
#include <gtest/gtest.h>

#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/MemoryLocalStore.h"
#include "eden/fs/testharness/TestUtil.h"
#include "eden/fs/utils/PathFuncs.h"

using namespace facebook::eden;
using folly::io::Cursor;

namespace {
class FakeBackingStoreTest : public ::testing::Test {
 protected:
  void SetUp() override {
    localStore_ = std::make_shared<MemoryLocalStore>();
    store_ = std::make_unique<FakeBackingStore>(localStore_);
  }

  void TearDown() override {
    store_.reset();
    localStore_.reset();
  }

  std::shared_ptr<LocalStore> localStore_;
  std::unique_ptr<FakeBackingStore> store_;
};

/**
 * Helper function to get blob contents as a string.
 *
 * We unfortunately can't use moveToFbString() or coalesce() since the Blob's
 * contents are always const.
 */
std::string blobContents(const Blob& blob) {
  Cursor c(&blob.getContents());
  return c.readFixedString(blob.getContents().computeChainDataLength());
}
} // namespace

TEST_F(FakeBackingStoreTest, getNonExistent) {
  // getTree()/getBlob()/getTreeForCommit() should throw immediately
  // when called on non-existent objects.
  auto hash = makeTestHash("1");
  EXPECT_THROW_RE(
      store_->getBlob(hash), std::domain_error, "blob 0+1 not found");
  EXPECT_THROW_RE(
      store_->getTree(hash), std::domain_error, "tree 0+1 not found");
  EXPECT_THROW_RE(
      store_->getTreeForCommit(hash),
      std::domain_error,
      "commit 0+1 not found");
}

TEST_F(FakeBackingStoreTest, getBlob) {
  // Add a blob to the tree
  auto hash = makeTestHash("1");
  auto* storedBlob = store_->putBlob(hash, "foobar");
  EXPECT_EQ(hash, storedBlob->get().getHash());
  EXPECT_EQ("foobar", blobContents(storedBlob->get()));

  // The blob is not ready yet, so calling getBlob() should yield not-ready
  // Future objects.
  auto future1 = store_->getBlob(hash);
  EXPECT_FALSE(future1.isReady());
  auto future2 = store_->getBlob(hash);
  EXPECT_FALSE(future2.isReady());

  // Calling trigger() should make the pending futures ready.
  storedBlob->trigger();
  ASSERT_TRUE(future1.isReady());
  ASSERT_TRUE(future2.isReady());
  EXPECT_EQ("foobar", blobContents(*std::move(future1).get()));
  EXPECT_EQ("foobar", blobContents(*std::move(future2).get()));

  // But subsequent calls to getBlob() should still yield unready futures.
  auto future3 = store_->getBlob(hash);
  EXPECT_FALSE(future3.isReady());
  auto future4 = store_->getBlob(hash);
  EXPECT_FALSE(future4.isReady());
  bool future4Failed = false;
  folly::exception_wrapper future4Error;
  std::move(future4)
      .thenValue([](auto&&) { FAIL() << "future4 should not succeed\n"; })
      .thenError([&](const folly::exception_wrapper& ew) {
        future4Failed = true;
        future4Error = ew;
      });

  // Calling triggerError() should fail pending futures
  storedBlob->triggerError(std::logic_error("does not compute"));
  ASSERT_TRUE(future3.isReady());
  EXPECT_THROW_RE(
      std::move(future3).get(), std::logic_error, "does not compute");
  ASSERT_TRUE(future4Failed);
  EXPECT_THROW_RE(
      future4Error.throw_exception(), std::logic_error, "does not compute");

  // Calling setReady() should make the pending futures ready, as well
  // as all subsequent Futures returned by getBlob()
  auto future5 = store_->getBlob(hash);
  EXPECT_FALSE(future5.isReady());

  storedBlob->setReady();
  ASSERT_TRUE(future5.isReady());
  EXPECT_EQ("foobar", blobContents(*std::move(future5).get()));

  // Subsequent calls to getBlob() should return Futures that are immediately
  // ready since we called setReady() above.
  auto future6 = store_->getBlob(hash);
  ASSERT_TRUE(future6.isReady());
  EXPECT_EQ("foobar", blobContents(*std::move(future6).get()));
}

TEST_F(FakeBackingStoreTest, getTree) {
  // Populate some files and directories in the store
  auto* runme = store_->putBlob("#!/bin/sh\necho 'hello world!'\n");
  auto* foo = store_->putBlob(makeTestHash("f00"), "this is foo\n");
  EXPECT_EQ(makeTestHash("f00"), foo->get().getHash());
  auto* bar = store_->putBlob("barbarbarbar\n");

  auto* dir1 = store_->putTree(
      makeTestHash("abc"),
      {
          {"foo", foo},
          {"runme", runme, FakeBlobType::EXECUTABLE_FILE},
      });
  EXPECT_EQ(makeTestHash("abc"), dir1->get().getHash());
  auto* dir2 = store_->putTree({{"README", store_->putBlob("docs go here")}});

  auto rootHash = makeTestHash("10101010");
  auto* rootDir = store_->putTree(
      rootHash,
      {
          {"zzz", foo, FakeBlobType::REGULAR_FILE},
          {"dir1", dir1},
          {"readonly", dir2},
          {"bar", bar},
      });

  // Try getting the root tree but failing it with triggerError()
  auto future1 = store_->getTree(rootHash);
  EXPECT_FALSE(future1.isReady());
  rootDir->triggerError(std::runtime_error("cosmic rays"));
  EXPECT_THROW_RE(std::move(future1).get(), std::runtime_error, "cosmic rays");

  // Now try using trigger()
  auto future2 = store_->getTree(rootHash);
  EXPECT_FALSE(future2.isReady());
  auto future3 = store_->getTree(rootHash);
  EXPECT_FALSE(future3.isReady());
  rootDir->trigger();
  ASSERT_TRUE(future2.isReady());
  ASSERT_TRUE(future3.isReady());

  auto tree2 = std::move(future2).get();
  EXPECT_EQ(rootHash, tree2->getHash());
  EXPECT_EQ(4, tree2->getTreeEntries().size());
  // Tree entries are always alphabetically sorted so we don't have to worry
  // about order here
  EXPECT_EQ("bar"_pc, tree2->getEntryAt(0).getName());
  EXPECT_EQ(bar->get().getHash(), tree2->getEntryAt(0).getHash());
  EXPECT_EQ(TreeEntryType::REGULAR_FILE, tree2->getEntryAt(0).getType());
  EXPECT_EQ("dir1"_pc, tree2->getEntryAt(1).getName());
  EXPECT_EQ(dir1->get().getHash(), tree2->getEntryAt(1).getHash());
  EXPECT_EQ(TreeEntryType::TREE, tree2->getEntryAt(1).getType());
  EXPECT_EQ("readonly"_pc, tree2->getEntryAt(2).getName());
  EXPECT_EQ(dir2->get().getHash(), tree2->getEntryAt(2).getHash());
  // TreeEntry objects only tracking the owner executable bit, so even though we
  // input the permissions as 0500 above this really ends up returning 0755
  EXPECT_EQ(TreeEntryType::TREE, tree2->getEntryAt(2).getType());
  EXPECT_EQ("zzz"_pc, tree2->getEntryAt(3).getName());
  EXPECT_EQ(foo->get().getHash(), tree2->getEntryAt(3).getHash());
  EXPECT_EQ(TreeEntryType::REGULAR_FILE, tree2->getEntryAt(3).getType());

  EXPECT_EQ(rootHash, std::move(future3).get()->getHash());

  // Now try using setReady()
  auto future4 = store_->getTree(rootHash);
  EXPECT_FALSE(future4.isReady());
  rootDir->setReady();
  ASSERT_TRUE(future4.isReady());
  EXPECT_EQ(rootHash, std::move(future4).get()->getHash());

  auto future5 = store_->getTree(rootHash);
  ASSERT_TRUE(future5.isReady());
  EXPECT_EQ(rootHash, std::move(future5).get()->getHash());
}

TEST_F(FakeBackingStoreTest, getTreeForCommit) {
  // Set up one commit with a root tree
  auto dir1Hash = makeTestHash("abc");
  auto* dir1 = store_->putTree(dir1Hash, {{"foo", store_->putBlob("foo\n")}});
  auto hash1 = makeTestHash("1");
  auto* commit1 = store_->putCommit(hash1, dir1);
  // Set up a second commit, but don't actually add the tree object for this one
  auto hash2 = makeTestHash("2");
  auto* commit2 = store_->putCommit(hash2, makeTestHash("3"));

  auto future1 = store_->getTreeForCommit(hash1);
  EXPECT_FALSE(future1.isReady());
  auto future2 = store_->getTreeForCommit(hash2);
  EXPECT_FALSE(future2.isReady());

  // Trigger commit1, then dir1 to make future1 ready.
  commit1->trigger();
  EXPECT_FALSE(future1.isReady());
  dir1->trigger();
  ASSERT_TRUE(future1.isReady());
  EXPECT_EQ(dir1Hash, std::move(future1).get()->getHash());

  // future2 should still be pending
  EXPECT_FALSE(future2.isReady());

  // Get another future for commit1
  auto future3 = store_->getTreeForCommit(hash1);
  EXPECT_FALSE(future3.isReady());
  // Triggering the directory now should have no effect,
  // since there should be no futures for it yet.
  dir1->trigger();
  EXPECT_FALSE(future3.isReady());
  commit1->trigger();
  EXPECT_FALSE(future3.isReady());
  dir1->trigger();
  ASSERT_TRUE(future3.isReady());
  EXPECT_EQ(dir1Hash, std::move(future3).get()->getHash());

  // Try triggering errors
  auto future4 = store_->getTreeForCommit(hash1);
  EXPECT_FALSE(future4.isReady());
  commit1->triggerError(std::runtime_error("bad luck"));
  ASSERT_TRUE(future4.isReady());
  EXPECT_THROW_RE(std::move(future4).get(), std::runtime_error, "bad luck");

  auto future5 = store_->getTreeForCommit(hash1);
  EXPECT_FALSE(future5.isReady());
  commit1->trigger();
  EXPECT_FALSE(future5.isReady());
  dir1->triggerError(std::runtime_error("PC Load Letter"));
  ASSERT_TRUE(future5.isReady());
  EXPECT_THROW_RE(
      std::move(future5).get(), std::runtime_error, "PC Load Letter");

  // Now trigger commit2.
  // This should trigger future2 to fail since the tree does not actually exist.
  commit2->trigger();
  ASSERT_TRUE(future2.isReady());
  EXPECT_THROW_RE(
      std::move(future2).get(),
      std::domain_error,
      "tree .* for commit .* not found");
}

TEST_F(FakeBackingStoreTest, getTreeForManifest) {
  // Set up one commit with a root tree
  auto dir1Hash = makeTestHash("abc");
  auto* dir1 = store_->putTree(dir1Hash, {{"foo", store_->putBlob("foo\n")}});
  auto hash1 = makeTestHash("1");
  auto* commit1 = store_->putCommit(hash1, dir1);
  // Set up a second commit, but don't actually add the tree object for this one
  auto hash2 = makeTestHash("2");
  auto hash3 = makeTestHash("3");
  store_->putCommit(hash2, hash3);

  auto future1 = store_->getTreeForManifest(hash1, dir1Hash);
  // Since we are not looking up the commit hash, but since the tree exists, we
  // are waiting on the tree to be triggered
  EXPECT_FALSE(future1.isReady());

  auto future2 = store_->getTreeForManifest(hash2, hash3);
  // Since we are not looking up the commit hash, and since the tree does not
  // exist, we are not waiting on the tree to be triggered to resolve this.
  EXPECT_TRUE(future2.isReady());
  // This should cause future2 to fail since the tree does not actually exist.
  EXPECT_THROW_RE(
      std::move(future2).get(),
      std::domain_error,
      "tree .* for commit .* not found");

  // Trigger the tree, the first future should be ready.
  dir1->trigger();
  ASSERT_TRUE(future1.isReady());
  EXPECT_EQ(dir1Hash, std::move(future1).get()->getHash());

  // Get another future for commit1
  auto future3 = store_->getTreeForManifest(hash1, dir1Hash);
  EXPECT_FALSE(future3.isReady());
  // Triggering the directory now should have an effect since we are not waiting
  // to look up the manifest id using the commit hash and are only waiting on
  // the directory
  dir1->trigger();
  EXPECT_TRUE(future3.isReady());
  EXPECT_EQ(dir1Hash, std::move(future3).get()->getHash());

  // Try triggering errors
  auto future4 = store_->getTreeForManifest(hash1, dir1Hash);
  EXPECT_FALSE(future4.isReady());
  commit1->triggerError(std::runtime_error("bad luck"));
  EXPECT_FALSE(future4.isReady());
  dir1->trigger();
  EXPECT_TRUE(future4.isReady());
  // Since getTreeForManifest does not depend on the commit to get the tree, and
  // since the tree is already in the store, an error being triggered with the
  // commit will not cause getTreeForManifest to fail
  EXPECT_EQ(dir1Hash, std::move(future4).get()->getHash());

  auto future5 = store_->getTreeForManifest(hash1, dir1Hash);
  EXPECT_FALSE(future5.isReady());
  commit1->trigger();
  EXPECT_FALSE(future5.isReady());
  dir1->triggerError(std::runtime_error("PC Load Letter"));
  ASSERT_TRUE(future5.isReady());
  EXPECT_THROW_RE(
      std::move(future5).get(), std::runtime_error, "PC Load Letter");
}

TEST_F(FakeBackingStoreTest, maybePutBlob) {
  auto foo1 = store_->maybePutBlob("foo\n");
  EXPECT_TRUE(foo1.second);
  auto foo2 = store_->maybePutBlob("foo\n");
  EXPECT_FALSE(foo2.second);
  EXPECT_EQ(foo1.first, foo2.first);
}

TEST_F(FakeBackingStoreTest, maybePutTree) {
  auto* foo = store_->putBlob("foo\n");
  auto* bar = store_->putBlob("bar\n");

  auto dir1 = store_->maybePutTree({
      {"foo", foo},
      {"bar", bar},
  });
  EXPECT_TRUE(dir1.second);

  // Listing the entries in a different order should still
  // result in the same tree.
  auto dir2 = store_->maybePutTree({
      {"bar", bar},
      {"foo", foo},
  });
  EXPECT_FALSE(dir2.second);
  EXPECT_EQ(dir1.first, dir2.first);
}
