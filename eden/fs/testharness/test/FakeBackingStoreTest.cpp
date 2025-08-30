/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/testharness/FakeBackingStore.h"

#include <folly/executors/ManualExecutor.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/test/TestUtils.h>
#include <folly/testing/TestUtil.h>
#include <gtest/gtest.h>

#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/model/TestOps.h"
#include "eden/fs/testharness/TestUtil.h"

using namespace std::literals::chrono_literals;
using folly::io::Cursor;

namespace facebook::eden {
class FakeBackingStoreTest : public ::testing::Test {
 protected:
  void SetUp() override {
    store_ = std::make_unique<FakeBackingStore>(
        BackingStore::LocalStoreCachingPolicy::NoCaching);
  }

  void TearDown() override {
    store_.reset();
  }

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

TEST_F(FakeBackingStoreTest, getNonExistent) {
  // getRootTree()/getTree()/getBlob() should throw immediately
  // when called on non-existent objects.
  EXPECT_THROW_RE(
      store_->getRootTree(RootId{"1"}, ObjectFetchContext::getNullContext()),
      std::domain_error,
      "commit 1 not found");
  auto id = makeTestId("1");
  EXPECT_THROW_RE(
      store_->getBlob(id, ObjectFetchContext::getNullContext()),
      std::domain_error,
      "blob 0+1 not found");
  EXPECT_THROW_RE(
      store_->getTree(id, ObjectFetchContext::getNullContext()),
      std::domain_error,
      "tree 0+1 not found");
}

TEST_F(FakeBackingStoreTest, getBlob) {
  // Add a blob to the tree
  auto id = makeTestId("1");
  auto* storedBlob = store_->putBlob(id, "foobar");
  EXPECT_EQ("foobar", blobContents(storedBlob->get()));

  auto executor = folly::ManualExecutor();

  // The blob is not ready yet, so calling getBlob() should yield not-ready
  // Future objects.
  auto future1 =
      store_->getBlob(id, ObjectFetchContext::getNullContext()).via(&executor);
  executor.drain();
  EXPECT_FALSE(future1.isReady());
  auto future2 =
      store_->getBlob(id, ObjectFetchContext::getNullContext()).via(&executor);
  executor.drain();
  EXPECT_FALSE(future2.isReady());

  // Calling trigger() should make the pending futures ready.
  storedBlob->trigger();
  executor.drain();
  ASSERT_TRUE(future1.isReady());
  ASSERT_TRUE(future2.isReady());
  EXPECT_EQ("foobar", blobContents(*std::move(future1).get(0ms).blob));
  EXPECT_EQ("foobar", blobContents(*std::move(future2).get(0ms).blob));

  // But subsequent calls to getBlob() should still yield unready futures.
  auto future3 =
      store_->getBlob(id, ObjectFetchContext::getNullContext()).via(&executor);
  EXPECT_FALSE(future3.isReady());
  auto future4 =
      store_->getBlob(id, ObjectFetchContext::getNullContext()).via(&executor);
  EXPECT_FALSE(future4.isReady());
  bool future4Failed = false;
  folly::exception_wrapper future4Error;

  std::move(future4)
      .via(&executor)
      .thenValue([](auto&&) { FAIL() << "future4 should not succeed\n"; })
      .thenError([&](const folly::exception_wrapper& ew) {
        future4Failed = true;
        future4Error = ew;
      });

  // Calling triggerError() should fail pending futures
  storedBlob->triggerError(std::logic_error("does not compute"));
  executor.drain();

  ASSERT_TRUE(future3.isReady());
  EXPECT_THROW_RE(
      std::move(future3).get(), std::logic_error, "does not compute");
  ASSERT_TRUE(future4Failed);
  EXPECT_THROW_RE(
      future4Error.throw_exception(), std::logic_error, "does not compute");

  // Calling setReady() should make the pending futures ready, as well
  // as all subsequent Futures returned by getBlob()
  auto future5 =
      store_->getBlob(id, ObjectFetchContext::getNullContext()).via(&executor);
  executor.drain();
  EXPECT_FALSE(future5.isReady());

  storedBlob->setReady();
  executor.drain();
  ASSERT_TRUE(future5.isReady());
  EXPECT_EQ("foobar", blobContents(*std::move(future5).get(0ms).blob));

  // Subsequent calls to getBlob() should return Futures that are immediately
  // ready since we called setReady() above.
  auto future6 =
      store_->getBlob(id, ObjectFetchContext::getNullContext()).via(&executor);
  executor.drain();
  ASSERT_TRUE(future6.isReady());
  EXPECT_EQ("foobar", blobContents(*std::move(future6).get(0ms).blob));
}

TEST_F(FakeBackingStoreTest, getTree) {
  // Populate some files and directories in the store
  auto [runme, runme_id] = store_->putBlob("#!/bin/sh\necho 'hello world!'\n");
  auto foo_id = makeTestId("f00");
  auto* foo = store_->putBlob(foo_id, "this is foo\n");
  auto [bar, bar_id] = store_->putBlob("barbarbarbar\n");

  (void)foo;

  auto* dir1 = store_->putTree(
      makeTestId("abc"),
      {
          {"foo", foo_id},
          {"runme", runme_id, FakeBlobType::EXECUTABLE_FILE},
      });
  EXPECT_EQ(makeTestId("abc"), dir1->get().getObjectId());
  auto* dir2 =
      store_->putTree({{"README", store_->putBlob("docs go here").second}});

  auto rootId = makeTestId("10101010");
  auto* rootDir = store_->putTree(
      rootId,
      {
          {"bar", bar_id},
          {"dir1", dir1},
          {"readonly", dir2},
          {"zzz", foo_id, FakeBlobType::REGULAR_FILE},
      });

  auto executor = folly::ManualExecutor();

  // Try getting the root tree but failing it with triggerError()
  auto future1 = store_->getTree(rootId, ObjectFetchContext::getNullContext())
                     .via(&executor);
  EXPECT_FALSE(future1.isReady());
  rootDir->triggerError(std::runtime_error("cosmic rays"));
  executor.drain();
  EXPECT_THROW_RE(
      std::move(future1).get(0ms), std::runtime_error, "cosmic rays");

  // Now try using trigger()
  auto future2 = store_->getTree(rootId, ObjectFetchContext::getNullContext())
                     .via(&executor);
  EXPECT_FALSE(future2.isReady());
  auto future3 = store_->getTree(rootId, ObjectFetchContext::getNullContext())
                     .via(&executor);
  EXPECT_FALSE(future3.isReady());
  rootDir->trigger();
  executor.drain();
  ASSERT_TRUE(future2.isReady());
  ASSERT_TRUE(future3.isReady());

  auto tree2 = std::move(future2).get(0ms).tree;
  EXPECT_EQ(rootId, tree2->getObjectId());
  EXPECT_EQ(4, tree2->size());

  auto [barName, barTreeEntry] = *tree2->find("bar"_pc);
  auto [dir1Name, dir1TreeEntry] = *tree2->find("dir1"_pc);
  auto [readonlyName, readonlyTreeEntry] = *tree2->find("readonly"_pc);
  auto [zzzName, zzzTreeEntry] = *tree2->find("zzz"_pc);
  EXPECT_EQ("bar"_pc, barName);
  EXPECT_EQ(TreeEntryType::REGULAR_FILE, barTreeEntry.getType());
  EXPECT_EQ(bar_id, barTreeEntry.getObjectId());
  EXPECT_EQ("dir1"_pc, dir1Name);
  EXPECT_EQ(dir1->get().getObjectId(), dir1TreeEntry.getObjectId());
  EXPECT_EQ(TreeEntryType::TREE, dir1TreeEntry.getType());
  EXPECT_EQ("readonly"_pc, readonlyName);
  EXPECT_EQ(dir2->get().getObjectId(), readonlyTreeEntry.getObjectId());
  // TreeEntry objects only tracking the owner executable bit, so even though we
  // input the permissions as 0500 above this really ends up returning 0755
  EXPECT_EQ(TreeEntryType::TREE, readonlyTreeEntry.getType());
  EXPECT_EQ("zzz"_pc, zzzName);
  EXPECT_EQ(foo_id, zzzTreeEntry.getObjectId());
  EXPECT_EQ(TreeEntryType::REGULAR_FILE, zzzTreeEntry.getType());

  EXPECT_EQ(rootId, std::move(future3).get(0ms).tree->getObjectId());

  // Now try using setReady()
  auto future4 = store_->getTree(rootId, ObjectFetchContext::getNullContext())
                     .via(&executor);
  EXPECT_FALSE(future4.isReady());
  rootDir->setReady();
  executor.drain();
  ASSERT_TRUE(future4.isReady());
  EXPECT_EQ(rootId, std::move(future4).get(0ms).tree->getObjectId());

  auto future5 = store_->getTree(rootId, ObjectFetchContext::getNullContext())
                     .via(&executor);
  executor.drain();
  ASSERT_TRUE(future5.isReady());
  EXPECT_EQ(rootId, std::move(future5).get(0ms).tree->getObjectId());
}

TEST_F(FakeBackingStoreTest, getRootTree) {
  // Set up one commit with a root tree
  auto dir1Id = makeTestId("abc");
  auto* dir1 =
      store_->putTree(dir1Id, {{"foo", store_->putBlob("foo\n").second}});
  auto* commit1 = store_->putCommit(RootId{"1"}, dir1);
  // Set up a second commit, but don't actually add the tree object for this one
  auto* commit2 = store_->putCommit(RootId{"2"}, makeTestId("3"));

  auto executor = folly::ManualExecutor();

  auto future1 =
      store_->getRootTree(RootId{"1"}, ObjectFetchContext::getNullContext())
          .semi()
          .via(&executor);
  EXPECT_FALSE(future1.isReady());
  auto future2 =
      store_->getRootTree(RootId{"2"}, ObjectFetchContext::getNullContext())
          .semi()
          .via(&executor);
  EXPECT_FALSE(future2.isReady());

  // Trigger commit1, then dir1 to make future1 ready.
  commit1->trigger();
  executor.drain();
  EXPECT_FALSE(future1.isReady());
  dir1->trigger();
  executor.drain();
  EXPECT_EQ(dir1Id, std::move(future1).get(0ms).treeId);

  // future2 should still be pending
  EXPECT_FALSE(future2.isReady());

  // Get another future for commit1
  auto future3 =
      store_->getRootTree(RootId{"1"}, ObjectFetchContext::getNullContext())
          .semi()
          .via(&executor);
  EXPECT_FALSE(future3.isReady());
  // Triggering the directory now should have no effect,
  // since there should be no futures for it yet.
  dir1->trigger();
  executor.drain();
  EXPECT_FALSE(future3.isReady());
  commit1->trigger();
  executor.drain();
  EXPECT_FALSE(future3.isReady());
  dir1->trigger();
  executor.drain();
  EXPECT_EQ(dir1Id, std::move(future3).get(0ms).treeId);

  // Try triggering errors
  auto future4 =
      store_->getRootTree(RootId{"1"}, ObjectFetchContext::getNullContext())
          .semi()
          .via(&executor);
  EXPECT_FALSE(future4.isReady());
  commit1->triggerError(std::runtime_error("bad luck"));
  executor.drain();
  EXPECT_THROW_RE(std::move(future4).get(0ms), std::runtime_error, "bad luck");

  auto future5 =
      store_->getRootTree(RootId{"1"}, ObjectFetchContext::getNullContext())
          .semi()
          .via(&executor);
  EXPECT_FALSE(future5.isReady());
  commit1->trigger();
  executor.drain();
  EXPECT_FALSE(future5.isReady());
  dir1->triggerError(std::runtime_error("PC Load Letter"));
  executor.drain();
  EXPECT_THROW_RE(
      std::move(future5).get(0ms), std::runtime_error, "PC Load Letter");

  // Now trigger commit2.
  // This should trigger future2 to fail since the tree does not actually exist.
  commit2->trigger();
  executor.drain();
  EXPECT_THROW_RE(
      std::move(future2).get(0ms),
      std::domain_error,
      "tree .* for commit .* not found");
}

TEST_F(FakeBackingStoreTest, maybePutBlob) {
  auto [foo1, foo1_id, foo1_inserted] = store_->maybePutBlob("foo\n");
  EXPECT_TRUE(foo1_inserted);
  auto [foo2, foo2_id, foo2_inserted] = store_->maybePutBlob("foo\n");
  EXPECT_FALSE(foo2_inserted);
  EXPECT_EQ(foo1, foo2);
}

TEST_F(FakeBackingStoreTest, maybePutTree) {
  auto [foo, foo_id] = store_->putBlob("foo\n");
  auto [bar, bar_id] = store_->putBlob("bar\n");

  auto dir1 = store_->maybePutTree({
      {"foo", foo_id},
      {"bar", bar_id},
  });
  EXPECT_TRUE(dir1.second);

  // Listing the entries in a different order should still
  // result in the same tree.
  auto dir2 = store_->maybePutTree({
      {"bar", bar_id},
      {"foo", foo_id},
  });
  EXPECT_FALSE(dir2.second);
  EXPECT_EQ(dir1.first, dir2.first);
}
} // namespace facebook::eden
