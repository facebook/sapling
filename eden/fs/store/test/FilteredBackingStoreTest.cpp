/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/testharness/FakeBackingStore.h"

#include <folly/Varint.h>
#include <folly/executors/ManualExecutor.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/test/TestUtils.h>
#include <folly/testing/TestUtil.h>
#include <gtest/gtest.h>

#include "eden/common/telemetry/NullStructuredLogger.h"
#include "eden/common/utils/FaultInjector.h"
#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/model/TestOps.h"
#include "eden/fs/store/BackingStoreLogger.h"
#include "eden/fs/store/FilteredBackingStore.h"
#include "eden/fs/store/MemoryLocalStore.h"
#include "eden/fs/store/filter/HgSparseFilter.h"
#include "eden/fs/store/hg/SaplingBackingStore.h"
#include "eden/fs/store/hg/SaplingBackingStoreOptions.h"
#include "eden/fs/testharness/FakeFilter.h"
#include "eden/fs/testharness/HgRepo.h"
#include "eden/fs/testharness/TestUtil.h"

namespace facebook::eden {

using namespace std::literals::chrono_literals;
using folly::io::Cursor;

const char kTestFilter1[] = "foo";
const char kTestFilter2[] = "football2";
const char kTestFilter3[] = "football3";
const char kTestFilter4[] = "shouldFilterZeroObjects";
const char kTestFilter5[] = "bazbar";
const char kTestFilter6[] =
    "\
[include]\n\
*\n\
[exclude]\n\
foo\n\
dir2/README\n\
filtered_out";
const char kTestFilter7[] = "dir2/README";
const char kTestFilter8[] = "this/filter/is/very/nested";

struct TestRepo {
  folly::test::TemporaryDirectory testDir{"eden_filtered_backing_store_test"};
  AbsolutePath testPath = canonicalPath(testDir.path().string());
  HgRepo repo{testPath + "repo"_pc};
  RootId commit1;
  Hash20 manifest1;

  TestRepo() {
    repo.hgInit(testPath + "cache"_pc);

    // Filtered out by kTestFilter6
    repo.mkdir("foo");
    repo.writeFile("foo/bar.txt", "filtered out\n");
    repo.mkdir("dir2");
    repo.writeFile("dir2/README", "filtered out again\n");
    repo.writeFile("filtered_out", "filtered out last\n");

    // Not filtered out by kTestFilter6
    repo.mkdir("src");
    repo.writeFile("src/hello.txt", "world\n");
    repo.writeFile("foo.txt", "foo\n");
    repo.writeFile("bar.txt", "bar\n");
    repo.writeFile("filter", kTestFilter6);
    repo.hg("add");
    commit1 = repo.commit("Initial commit");
    manifest1 = repo.getManifestForCommit(commit1);
  }
};

class FakeSubstringFilteredBackingStoreTest : public ::testing::Test {
 protected:
  void SetUp() override {
    wrappedStore_ = std::make_shared<FakeBackingStore>(
        BackingStore::LocalStoreCachingPolicy::Anything);
    auto fakeFilter = std::make_unique<FakeSubstringFilter>();
    filteredStore_ = std::make_shared<FilteredBackingStore>(
        wrappedStore_, std::move(fakeFilter));
  }

  void TearDown() override {
    filteredStore_.reset();
  }

  std::shared_ptr<FakeBackingStore> wrappedStore_;
  std::shared_ptr<FilteredBackingStore> filteredStore_;
};

class FakePrefixFilteredBackingStoreTest : public ::testing::Test {
 protected:
  void SetUp() override {
    wrappedStore_ = std::make_shared<FakeBackingStore>(
        BackingStore::LocalStoreCachingPolicy::Anything);
    auto fakeFilter = std::make_unique<FakePrefixFilter>();
    filteredStore_ = std::make_shared<FilteredBackingStore>(
        wrappedStore_, std::move(fakeFilter));
  }

  void TearDown() override {
    filteredStore_.reset();
  }

  std::shared_ptr<FakeBackingStore> wrappedStore_;
  std::shared_ptr<FilteredBackingStore> filteredStore_;
};

struct SaplingFilteredBackingStoreTest : TestRepo, ::testing::Test {
  SaplingFilteredBackingStoreTest() = default;

  void SetUp() override {
    auto hgFilter = std::make_unique<HgSparseFilter>(repo.path().copy());
    filteredStoreFFI_ = std::make_shared<FilteredBackingStore>(
        wrappedStore_, std::move(hgFilter));
  }

  void TearDown() override {
    filteredStoreFFI_.reset();
  }

  std::shared_ptr<ReloadableConfig> edenConfig{
      std::make_shared<ReloadableConfig>(EdenConfig::createTestEdenConfig())};
  EdenStatsPtr stats{makeRefPtr<EdenStats>()};
  std::shared_ptr<MemoryLocalStore> localStore{
      std::make_shared<MemoryLocalStore>(stats.copy())};

  std::shared_ptr<FilteredBackingStore> filteredStoreFFI_;

  FaultInjector faultInjector{/*enabled=*/false};

  std::unique_ptr<SaplingBackingStoreOptions> runtimeOptions =
      std::make_unique<SaplingBackingStoreOptions>();

  folly::InlineExecutor executor_ = folly::InlineExecutor::instance();

  std::shared_ptr<SaplingBackingStore> wrappedStore_{
      std::make_shared<SaplingBackingStore>(
          repo.path(),
          repo.path(),
          localStore,
          stats.copy(),
          &executor_,
          edenConfig,
          std::move(runtimeOptions),
          std::make_shared<NullStructuredLogger>(),
          std::make_unique<BackingStoreLogger>(),
          &faultInjector)};
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

TEST_F(FakeSubstringFilteredBackingStoreTest, getNonExistent) {
  // getRootTree()/getTree()/getBlob() should throw immediately
  // when called on non-existent objects.
  EXPECT_THROW_RE(
      filteredStore_->getRootTree(
          RootId{FilteredBackingStore::createFilteredRootId("1", kTestFilter1)},
          ObjectFetchContext::getNullContext()),
      std::domain_error,
      "commit 1 not found");
  auto id = makeTestId("1");
  auto blobFilterId =
      FilteredObjectId(id, FilteredObjectIdType::OBJECT_TYPE_BLOB);
  EXPECT_THROW_RE(
      filteredStore_->getBlob(
          ObjectId{blobFilterId.getValue()},
          ObjectFetchContext::getNullContext()),
      std::domain_error,
      "blob 0.*1 not found");
  auto relPath = RelativePathPiece{"foo/bar"};
  auto treeFilterId = FilteredObjectId(relPath, kTestFilter1, id);
  EXPECT_THROW_RE(
      filteredStore_->getTree(
          ObjectId{treeFilterId.getValue()},
          ObjectFetchContext::getNullContext()),
      std::domain_error,
      "tree 0.*1 not found");
}

TEST_F(FakeSubstringFilteredBackingStoreTest, getBlob) {
  // Add a blob to the tree
  auto id = makeTestId("1");
  auto filteredId = ObjectId{
      FilteredObjectId{id, FilteredObjectIdType::OBJECT_TYPE_BLOB}.getValue()};
  auto* storedBlob = wrappedStore_->putBlob(id, "foobar");
  EXPECT_EQ("foobar", blobContents(storedBlob->get()));

  auto executor = folly::ManualExecutor();

  // The blob is not ready yet, so calling getBlob() should yield not-ready
  // Future objects.
  auto future1 =
      filteredStore_->getBlob(filteredId, ObjectFetchContext::getNullContext())
          .via(&executor);
  EXPECT_FALSE(future1.isReady());
  auto future2 =
      filteredStore_->getBlob(filteredId, ObjectFetchContext::getNullContext())
          .via(&executor);
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
      filteredStore_->getBlob(filteredId, ObjectFetchContext::getNullContext())
          .via(&executor);
  EXPECT_FALSE(future3.isReady());
  auto future4 =
      filteredStore_->getBlob(filteredId, ObjectFetchContext::getNullContext())
          .via(&executor);
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

  executor.drain();
  // Calling triggerError() should fail pending futures
  storedBlob->triggerError(std::logic_error("does not compute"));
  executor.drain();

  ASSERT_TRUE(future3.isReady());
  EXPECT_THROW_RE(
      std::move(future3).get(0ms), std::logic_error, "does not compute");
  ASSERT_TRUE(future4Failed);
  EXPECT_THROW_RE(
      future4Error.throw_exception(), std::logic_error, "does not compute");

  // Calling setReady() should make the pending futures ready, as well
  // as all subsequent Futures returned by getBlob()
  auto future5 =
      filteredStore_->getBlob(filteredId, ObjectFetchContext::getNullContext())
          .via(&executor);
  EXPECT_FALSE(future5.isReady());

  storedBlob->setReady();
  executor.drain();
  ASSERT_TRUE(future5.isReady());
  EXPECT_EQ("foobar", blobContents(*std::move(future5).get(0ms).blob));

  // Subsequent calls to getBlob() should return Futures that are immediately
  // ready since we called setReady() above.
  auto future6 =
      filteredStore_->getBlob(filteredId, ObjectFetchContext::getNullContext())
          .via(&executor);
  executor.drain();
  ASSERT_TRUE(future6.isReady());
  EXPECT_EQ("foobar", blobContents(*std::move(future6).get(0ms).blob));
}

TEST_F(FakeSubstringFilteredBackingStoreTest, getTree) {
  // Populate some files in the store
  auto [runme, runme_id] =
      wrappedStore_->putBlob("#!/bin/sh\necho 'hello world!'\n");
  auto foo_id = makeTestId("f00");
  (void)wrappedStore_->putBlob(foo_id, "this is foo\n");
  auto [bar, bar_id] = wrappedStore_->putBlob("barbarbarbar\n");

  // Populate a couple directories as well
  auto* dir1 = wrappedStore_->putTree(
      makeTestId("abc"),
      {
          // "foo" will be filtered once the filter is applied
          {"foo", foo_id},
          {"runme", runme_id, FakeBlobType::EXECUTABLE_FILE},
      });
  EXPECT_EQ(makeTestId("abc"), dir1->get().getObjectId());
  auto* dir2 = wrappedStore_->putTree(
      {{"README", wrappedStore_->putBlob("docs go here")}});

  // Create a root directory and populate the root tree
  auto rootId = makeTestId("10101010");
  auto treeId = FilteredObjectId(RelativePath{""}, kTestFilter1, rootId);
  auto treeOID = ObjectId{treeId.getValue()};
  auto* rootDir = wrappedStore_->putTree(
      rootId,
      {
          {"bar", bar_id},
          {"dir1", dir1},
          {"readonly", dir2},
          {"zzz", foo_id, FakeBlobType::REGULAR_FILE},
          // this "foo" will also be filtered once the filter is applied.
          {"foo", foo_id, FakeBlobType::REGULAR_FILE},
      });

  // Try getting the root tree but fail it with triggerError()
  auto future1 =
      filteredStore_->getTree(treeOID, ObjectFetchContext::getNullContext());
  EXPECT_FALSE(future1.isReady());
  rootDir->triggerError(std::runtime_error("cosmic rays"));
  EXPECT_THROW_RE(
      std::move(future1).get(0ms), std::runtime_error, "cosmic rays");

  // Now try using trigger()
  auto future2 =
      filteredStore_->getTree(treeOID, ObjectFetchContext::getNullContext());
  EXPECT_FALSE(future2.isReady());
  auto future3 =
      filteredStore_->getTree(treeOID, ObjectFetchContext::getNullContext());
  EXPECT_FALSE(future3.isReady());
  rootDir->trigger();

  // Get the root tree object from the future
  auto tree2 = std::move(future2).get(0ms).tree;
  EXPECT_EQ(treeOID, tree2->getObjectId());
  EXPECT_EQ(4, tree2->size());

  // Get the tree entries for the root tree
  auto [barName, barTreeEntry] = *tree2->find("bar"_pc);
  auto [dir1Name, dir1TreeEntry] = *tree2->find("dir1"_pc);
  auto [readonlyName, readonlyTreeEntry] = *tree2->find("readonly"_pc);
  auto [zzzName, zzzTreeEntry] = *tree2->find("zzz"_pc);

  // We expect foo to be filtered from the root tree
  auto fooFindRes = tree2->find("foo"_pc);
  EXPECT_EQ(fooFindRes, tree2->cend());

  // Get the subtree for dir1
  auto dir1FOID = FilteredObjectId(
      RelativePath{"dir1"}, kTestFilter1, dir1->get().getObjectId());
  auto subTreefuture = filteredStore_->getTree(
      ObjectId{dir1FOID.getValue()}, ObjectFetchContext::getNullContext());
  dir1->trigger();
  auto subTree = std::move(subTreefuture).get(0ms).tree;

  // We expect runme to exist in the subtree
  auto [runmeName, runmeTreeEntry] = *subTree->find("runme"_pc);
  EXPECT_EQ("runme"_pc, runmeName);
  auto runmeFOID =
      FilteredObjectId(runme_id, FilteredObjectIdType::OBJECT_TYPE_BLOB);
  if (folly::kIsWindows) {
    // Windows executables show up as regular files
    EXPECT_EQ(TreeEntryType::REGULAR_FILE, runmeTreeEntry.getType());
  } else {
    EXPECT_EQ(TreeEntryType::EXECUTABLE_FILE, runmeTreeEntry.getType());
  }
  EXPECT_EQ(runmeFOID.getValue(), runmeTreeEntry.getObjectId().asString());

  // We don't expect foo to be in the subtree. It should be filtered out.
  EXPECT_EQ(subTree->find("foo"_pc), subTree->cend());

  // Finally, test that all other entries in the root tree are valid.
  EXPECT_EQ("bar"_pc, barName);
  auto barFOID =
      FilteredObjectId(bar_id, FilteredObjectIdType::OBJECT_TYPE_BLOB);
  EXPECT_EQ(barFOID.getValue(), barTreeEntry.getObjectId().asString());
  EXPECT_EQ(TreeEntryType::REGULAR_FILE, barTreeEntry.getType());

  EXPECT_EQ("dir1"_pc, dir1Name);
  EXPECT_EQ(dir1FOID.getValue(), dir1TreeEntry.getObjectId().asString());
  EXPECT_EQ(TreeEntryType::TREE, dir1TreeEntry.getType());

  EXPECT_EQ("readonly"_pc, readonlyName);
  auto dir2FOID = FilteredObjectId{
      RelativePath{"readonly"}, kTestFilter1, dir2->get().getObjectId()};
  EXPECT_EQ(dir2FOID.getValue(), readonlyTreeEntry.getObjectId().asString());
  // TreeEntry objects only tracking the owner executable bit, so even though
  // we input the permissions as 0500 above this really ends up returning 0755
  EXPECT_EQ(TreeEntryType::TREE, readonlyTreeEntry.getType());

  EXPECT_EQ("zzz"_pc, zzzName);
  auto zzzFOID =
      FilteredObjectId{foo_id, FilteredObjectIdType::OBJECT_TYPE_BLOB};
  EXPECT_EQ(zzzFOID.getValue(), zzzTreeEntry.getObjectId().asString());
  EXPECT_EQ(TreeEntryType::REGULAR_FILE, zzzTreeEntry.getType());

  // We expect future3 to also contain the root tree object
  EXPECT_EQ(treeOID, std::move(future3).get(0ms).tree->getObjectId());

  // Now try using setReady()
  auto future4 =
      filteredStore_->getTree(treeOID, ObjectFetchContext::getNullContext());
  EXPECT_FALSE(future4.isReady());
  rootDir->setReady();
  EXPECT_EQ(treeOID, std::move(future4).get(0ms).tree->getObjectId());

  auto future5 =
      filteredStore_->getTree(treeOID, ObjectFetchContext::getNullContext());
  EXPECT_EQ(treeOID, std::move(future5).get(0ms).tree->getObjectId());
}

TEST_F(FakeSubstringFilteredBackingStoreTest, getRootTree) {
  // Set up one commit with a root tree
  auto dir1Id = makeTestId("abc");
  auto dir1FOID = FilteredObjectId(RelativePath{""}, kTestFilter1, dir1Id);
  auto* dir1 = wrappedStore_->putTree(
      dir1Id, {{"foo", wrappedStore_->putBlob("foo\n")}});
  auto* commit1 = wrappedStore_->putCommit(RootId{"1"}, dir1);
  // Set up a second commit, but don't actually add the tree object for this
  // one
  auto* commit2 = wrappedStore_->putCommit(RootId{"2"}, makeTestId("3"));

  auto executor = folly::ManualExecutor();

  auto future1 = filteredStore_
                     ->getRootTree(
                         RootId{FilteredBackingStore::createFilteredRootId(
                             "1", kTestFilter1)},
                         ObjectFetchContext::getNullContext())
                     .semi()
                     .via(&executor);
  EXPECT_FALSE(future1.isReady());
  auto future2 = filteredStore_
                     ->getRootTree(
                         RootId{FilteredBackingStore::createFilteredRootId(
                             "2", kTestFilter1)},
                         ObjectFetchContext::getNullContext())
                     .semi()
                     .via(&executor);
  EXPECT_FALSE(future2.isReady());

  // Trigger commit1, then dir1 to make future1 ready.
  commit1->trigger();
  executor.drain();
  EXPECT_FALSE(future1.isReady());
  dir1->trigger();
  executor.drain();
  EXPECT_EQ(ObjectId{dir1FOID.getValue()}, std::move(future1).get(0ms).treeId);

  // future2 should still be pending
  EXPECT_FALSE(future2.isReady());

  // Get another future for commit1
  auto future3 = filteredStore_
                     ->getRootTree(
                         RootId{FilteredBackingStore::createFilteredRootId(
                             "1", kTestFilter1)},
                         ObjectFetchContext::getNullContext())
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
  EXPECT_EQ(ObjectId{dir1FOID.getValue()}, std::move(future3).get().treeId);

  // Try triggering errors
  auto future4 = filteredStore_
                     ->getRootTree(
                         RootId{FilteredBackingStore::createFilteredRootId(
                             "1", kTestFilter1)},
                         ObjectFetchContext::getNullContext())
                     .semi()
                     .via(&executor);
  executor.drain();
  EXPECT_FALSE(future4.isReady());
  commit1->triggerError(std::runtime_error("bad luck"));
  executor.drain();
  EXPECT_THROW_RE(std::move(future4).get(0ms), std::runtime_error, "bad luck");

  auto future5 = filteredStore_
                     ->getRootTree(
                         RootId{FilteredBackingStore::createFilteredRootId(
                             "1", kTestFilter1)},
                         ObjectFetchContext::getNullContext())
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
  // This should trigger future2 to fail since the tree does not actually
  // exist.
  commit2->trigger();
  executor.drain();
  EXPECT_THROW_RE(
      std::move(future2).get(0ms),
      std::domain_error,
      "tree .* for commit .* not found");
}

TEST_F(FakeSubstringFilteredBackingStoreTest, testCompareBlobObjectsById) {
  // Populate some blobs for testing.
  //
  // NOTE: FakeBackingStore is very dumb and implements its
  // compareObjectsById function as a bytewise comparison of ids. Therefore,
  // in order for two blobs to be equal, their ids (NOT their contents) need
  // to be equal.
  auto foobarId = makeTestId("f00");
  (void)wrappedStore_->putBlob(foobarId, "foobar");
  auto footballId = makeTestId("f001ba11");
  (void)wrappedStore_->putBlob(footballId, "football");

  // populate some trees
  auto rootDirId = makeTestId("f00d");
  auto* rootDirTree = wrappedStore_->putTree(
      rootDirId,
      {
          {"foobar1", foobarId},
          {"foobar2", foobarId},
          {"football1", footballId},
          {"football2", footballId},
      });
  auto fooDirExtendedId = makeTestId("f00d1e");
  auto* fooDirExtendedTree = wrappedStore_->putTree(
      fooDirExtendedId,
      {
          {"foobar1", foobarId},
          {"foobar2", foobarId},
          {"foobar3", foobarId},
          {"football1", footballId},
          {"football2", footballId},
      });

  // Set up one commit with a root tree
  auto* commit1 = wrappedStore_->putCommit(RootId{"1"}, rootDirTree);
  // Set up a second commit with an additional file
  auto* commit2 = wrappedStore_->putCommit(RootId{"2"}, fooDirExtendedTree);

  auto executor = folly::ManualExecutor();

  auto future1 = filteredStore_
                     ->getRootTree(
                         RootId{FilteredBackingStore::createFilteredRootId(
                             "1", kTestFilter2)},
                         ObjectFetchContext::getNullContext())
                     .semi()
                     .via(&executor);
  auto future2 = filteredStore_
                     ->getRootTree(
                         RootId{FilteredBackingStore::createFilteredRootId(
                             "2", kTestFilter3)},
                         ObjectFetchContext::getNullContext())
                     .semi()
                     .via(&executor);

  // Trigger commit1, then rootDirTree to make future1 ready.
  commit1->trigger();
  executor.drain();
  EXPECT_FALSE(future1.isReady());
  rootDirTree->trigger();
  executor.drain();
  auto fooDirRes = std::move(future1).get(0ms);

  // Get the object IDs of all the blobs from commit 1.
  auto [foobar1Name1, foobar1TreeEntry1] = *fooDirRes.tree->find("foobar1"_pc);
  auto foobar1OID1 = foobar1TreeEntry1.getObjectId();
  auto [foobar2Name1, foobar2TreeEntry1] = *fooDirRes.tree->find("foobar2"_pc);
  auto foobar2OID1 = foobar2TreeEntry1.getObjectId();
  auto [football1Name1, football1TreeEntry1] =
      *fooDirRes.tree->find("football1"_pc);
  auto football1OID1 = football1TreeEntry1.getObjectId();

  // We expect all the foo blobs in commit 1 to NOT be filtered. Therefore, foos
  // should equal foos. Football2 is filtered, and therefore unavailable for
  // comparison.
  EXPECT_EQ(
      filteredStore_->compareObjectsById(foobar1OID1, foobar2OID1),
      ObjectComparison::Identical);
  EXPECT_EQ(
      filteredStore_->compareObjectsById(foobar2OID1, foobar1OID1),
      ObjectComparison::Identical);
  EXPECT_EQ(
      filteredStore_->compareObjectsById(football1OID1, football1OID1),
      ObjectComparison::Identical);
  EXPECT_NE(
      filteredStore_->compareObjectsById(football1OID1, foobar1OID1),
      ObjectComparison::Identical);
  EXPECT_NE(
      filteredStore_->compareObjectsById(foobar2OID1, football1OID1),
      ObjectComparison::Identical);

  // Trigger commit2, then rootDirTreeExtended to make future2 ready.
  commit2->trigger();
  executor.drain();
  fooDirExtendedTree->trigger();
  executor.drain();
  auto fooDirExtRes = std::move(future2).get(0ms);

  // Get the object IDs of all the blobs from commit 1.
  auto [foobar1Name2, foobar1TreeEntry2] =
      *fooDirExtRes.tree->find("foobar1"_pc);
  auto foobar1OID2 = foobar1TreeEntry2.getObjectId();
  auto [foobar2Name2, foobar2TreeEntry2] =
      *fooDirExtRes.tree->find("foobar2"_pc);
  auto foobar2OID2 = foobar2TreeEntry2.getObjectId();
  auto [football1Name2, football1TreeEntry2] =
      *fooDirExtRes.tree->find("football1"_pc);
  auto football1OID2 = football1TreeEntry2.getObjectId();
  auto [football2Name2, football2TreeEntry2] =
      *fooDirExtRes.tree->find("football2"_pc);
  auto football2OID2 = football2TreeEntry2.getObjectId();

  // Only football3 is unavailable for comparison in commit2. Let's make sure
  // all the corresponding blobs evaluate to equal even if they have different
  // filters.
  EXPECT_EQ(
      filteredStore_->compareObjectsById(foobar1OID1, foobar1OID2),
      ObjectComparison::Identical);
  EXPECT_EQ(
      filteredStore_->compareObjectsById(foobar2OID1, foobar1OID2),
      ObjectComparison::Identical);
  EXPECT_EQ(
      filteredStore_->compareObjectsById(football1OID1, football1OID2),
      ObjectComparison::Identical);
  EXPECT_EQ(
      filteredStore_->compareObjectsById(football1OID1, football2OID2),
      ObjectComparison::Identical);
  EXPECT_NE(
      filteredStore_->compareObjectsById(football1OID1, foobar1OID1),
      ObjectComparison::Identical);
  EXPECT_NE(
      filteredStore_->compareObjectsById(foobar2OID1, football2OID2),
      ObjectComparison::Identical);
}

TEST_F(FakeSubstringFilteredBackingStoreTest, testCompareTreeObjectsById) {
  // Populate some blobs for testing.
  //
  // NOTE: FakeBackingStore is very dumb and implements its
  // compareObjectsById function as a bytewise comparison of ids. Therefore,
  // in order for two blobs to be equal, their ids (NOT their contents) need
  // to be equal.
  auto foobarId = makeTestId("f00");
  (void)wrappedStore_->putBlob(foobarId, "foobar");
  auto footballId = makeTestId("f001ba11");
  (void)wrappedStore_->putBlob(footballId, "football");
  auto bazbarId = makeTestId("ba5ba4");
  (void)wrappedStore_->putBlob(bazbarId, "bazbar");
  auto bazballId = makeTestId("ba5ba11");
  (void)wrappedStore_->putBlob(bazballId, "bazball");

  // populate some trees
  auto grandchildTreeId = makeTestId("ba5");
  auto grandchildTree = wrappedStore_->putTree(
      grandchildTreeId,
      {
          {"bazbar", bazbarId},
          {"bazball", bazballId},
      });
  auto childTreeId = makeTestId("f00ba5");
  auto childTree =
      wrappedStore_->putTree(childTreeId, {{"grandchild", grandchildTree}});
  auto modifiedChildTreeId = makeTestId("f00ba52");
  auto modifiedChildTree = wrappedStore_->putTree(
      modifiedChildTreeId,
      {{"grandchild", grandchildTree}, {"newentry", foobarId}});
  auto rootDirId = makeTestId("f00d");
  auto* rootDirTree = wrappedStore_->putTree(
      rootDirId,
      {
          {"foobar1", foobarId},
          {"foobar2", foobarId},
          {"football1", footballId},
          {"football2", footballId},
          {"child", childTree},
      });

  auto modifiedRootDirId = makeTestId("f00e");
  auto* modifiedRootDirTree = wrappedStore_->putTree(
      modifiedRootDirId,
      {
          {"foobar1", foobarId},
          {"foobar2", foobarId},
          {"football1", footballId},
          {"football2", footballId},
          {"child", modifiedChildTree},
      });

  // Set up one commit with a root tree
  auto* commit1 = wrappedStore_->putCommit(RootId{"1"}, rootDirTree);
  // Set up a second commit with an additional file
  auto* commit2 = wrappedStore_->putCommit(RootId{"2"}, modifiedRootDirTree);

  auto executor = folly::ManualExecutor();

  auto rootFuture1 = filteredStore_
                         ->getRootTree(
                             RootId{FilteredBackingStore::createFilteredRootId(
                                 "1", kTestFilter4)},
                             ObjectFetchContext::getNullContext())
                         .semi()
                         .via(&executor);
  auto rootFuture2 = filteredStore_
                         ->getRootTree(
                             RootId{FilteredBackingStore::createFilteredRootId(
                                 "2", kTestFilter5)},
                             ObjectFetchContext::getNullContext())
                         .semi()
                         .via(&executor);

  // Trigger commit1, then rootDirTree to make rootFuture1 ready.
  commit1->trigger();
  executor.drain();
  EXPECT_FALSE(rootFuture1.isReady());
  rootDirTree->trigger();
  executor.drain();
  auto rootDirRes1 = std::move(rootFuture1).get(0ms);

  // Get the object IDs of all the trees from commit 1.
  auto [childName, childEntry] = *rootDirRes1.tree->find("child"_pc);
  auto childOID = childEntry.getObjectId();
  auto childFuture1 =
      filteredStore_->getTree(childOID, ObjectFetchContext::getNullContext());
  childTree->trigger();
  auto childDirRes1 = std::move(childFuture1).get(0ms).tree;
  auto [grandchildName, grandchildEntry] = *childDirRes1->find("grandchild"_pc);
  auto grandchildOID = grandchildEntry.getObjectId();

  // Trigger commit2, then rootDirTreeExtended to make rootFuture2 ready.
  commit2->trigger();
  executor.drain();
  modifiedRootDirTree->trigger();
  executor.drain();
  auto rootDirCommit2Res = std::move(rootFuture2).get(0ms);

  // Get the object IDs of all the blobs from commit 1.
  auto [childName2, childEntry2] = *rootDirCommit2Res.tree->find("child"_pc);
  auto childOID2 = childEntry2.getObjectId();
  auto childFuture2 =
      filteredStore_->getTree(childOID2, ObjectFetchContext::getNullContext());
  modifiedChildTree->trigger();
  auto childDirRes2 = std::move(childFuture2).get(0ms).tree;
  auto [grandchildName2, grandchildEntry2] =
      *childDirRes2->find("grandchild"_pc);
  auto grandchildOID2 = grandchildEntry2.getObjectId();

  // The child tree should know it changed between filters (since the actual
  // contents changed), BUT FakeBackingStore is dumb and can't determine that.
  // Therefore, this just returns unknown.
  EXPECT_EQ(
      filteredStore_->compareObjectsById(childOID, childOID2),
      ObjectComparison::Unknown);
  // The root tree didn't change, but its children might have. So it reports
  // Unknown.
  EXPECT_EQ(
      filteredStore_->compareObjectsById(
          rootDirRes1.tree->getObjectId(),
          rootDirCommit2Res.tree->getObjectId()),
      ObjectComparison::Unknown);
  // The root tree should be identical to itself
  EXPECT_EQ(
      filteredStore_->compareObjectsById(
          rootDirRes1.tree->getObjectId(), rootDirRes1.tree->getObjectId()),
      ObjectComparison::Identical);
  // The grandchild tree got filtered, but it isn't aware that its children were
  // filtered. We return Unknown in this case.
  EXPECT_TRUE(
      filteredStore_->compareObjectsById(grandchildOID, grandchildOID2) ==
      ObjectComparison::Unknown);
}

TEST_F(FakeSubstringFilteredBackingStoreTest, getGlobFiles) {
  // Populate the backing store glob files
  RootId rootId =
      RootId{FilteredBackingStore::createFilteredRootId("1", kTestFilter1)};
  RootId rootId2 =
      RootId{FilteredBackingStore::createFilteredRootId("2", kTestFilter2)};
  RootId rootId3 =
      RootId{FilteredBackingStore::createFilteredRootId("3", kTestFilter4)};
  RootId rootId4 =
      RootId{FilteredBackingStore::createFilteredRootId("4", kTestFilter7)};
  RootId rootId5 =
      RootId{FilteredBackingStore::createFilteredRootId("5", kTestFilter8)};
  wrappedStore_->putGlob(
      std::pair<RootId, std::string>{RootId{"1"}, "foo"},
      std::vector<std::string>{"football2", "football3", "foo/bar/baz.cpp"});
  wrappedStore_->putGlob(
      std::pair<RootId, std::string>{RootId{"2"}, "foo"},
      std::vector<std::string>{"football2", "football3", "foo/tball2/baz.cpp"});
  wrappedStore_->putGlob(
      std::pair<RootId, std::string>{RootId{"3"}, "foo"},
      std::vector<std::string>{"football2", "football3", "foo/bar/baz.cpp"});
  wrappedStore_->putGlob(
      std::pair<RootId, std::string>{RootId{"4"}, "foo"},
      std::vector<std::string>{
          "football2",
          "football3",
          "foo/bar/baz.cpp",
          "dir2/foo.txt",
          "dir2/foo/README",
          "dir2/README",
          "dir2/README.txt",
          "dir2/README2/read.txt",
      });
  wrappedStore_->putGlob(
      std::pair<RootId, std::string>{RootId{"5"}, "foo"},
      std::vector<std::string>{
          "this",
          "this/filter",
          "this/filter/is",
          "this/filter/is/very",
          "this/filter/is/very/nested",
      });

  // Get the glob files
  auto executor = folly::ManualExecutor();

  auto filteredFut1 = filteredStore_
                          ->getGlobFiles(
                              rootId,
                              std::vector<std::string>{"foo"},
                              std::vector<std::string>{})
                          .semi()
                          .via(&executor);
  auto filteredFut2 = filteredStore_
                          ->getGlobFiles(
                              rootId2,
                              std::vector<std::string>{"foo"},
                              std::vector<std::string>{})
                          .semi()
                          .via(&executor);
  auto filteredFut3 = filteredStore_
                          ->getGlobFiles(
                              rootId3,
                              std::vector<std::string>{"foo"},
                              std::vector<std::string>{})
                          .semi()
                          .via(&executor);
  auto filteredFut4 = filteredStore_
                          ->getGlobFiles(
                              rootId4,
                              std::vector<std::string>{"foo"},
                              std::vector<std::string>{})
                          .semi()
                          .via(&executor);
  auto filteredFut5 = filteredStore_
                          ->getGlobFiles(
                              rootId5,
                              std::vector<std::string>{"foo"},
                              std::vector<std::string>{})
                          .semi()
                          .via(&executor);
  executor.drain();
  EXPECT_TRUE(filteredFut1.isReady());
  EXPECT_TRUE(filteredFut2.isReady());
  EXPECT_TRUE(filteredFut3.isReady());
  EXPECT_TRUE(filteredFut4.isReady());
  EXPECT_TRUE(filteredFut5.isReady());

  auto filteredFutRes1 = std::move(filteredFut1).get(0ms);
  auto filteredFutRes2 = std::move(filteredFut2).get(0ms);
  auto filteredFutRes3 = std::move(filteredFut3).get(0ms);
  auto filteredFutRes4 = std::move(filteredFut4).get(0ms);
  auto filteredFutRes5 = std::move(filteredFut5).get(0ms);

  // Check that the glob files are filtered correctly
  EXPECT_EQ(filteredFutRes1.globFiles.size(), 0);
  EXPECT_EQ(filteredFutRes2.globFiles.size(), 2);
  EXPECT_EQ(filteredFutRes3.globFiles.size(), 3);
  EXPECT_EQ(filteredFutRes4.globFiles.size(), 5);
  EXPECT_EQ(filteredFutRes5.globFiles.size(), 4);

  EXPECT_EQ(filteredFutRes2.globFiles[0], "football3");
  EXPECT_EQ(filteredFutRes2.globFiles[1], "foo/tball2/baz.cpp");

  EXPECT_EQ(filteredFutRes3.globFiles[0], "football2");
  EXPECT_EQ(filteredFutRes3.globFiles[1], "football3");
  EXPECT_EQ(filteredFutRes3.globFiles[2], "foo/bar/baz.cpp");

  EXPECT_EQ(filteredFutRes4.globFiles[0], "football2");
  EXPECT_EQ(filteredFutRes4.globFiles[1], "football3");
  EXPECT_EQ(filteredFutRes4.globFiles[2], "foo/bar/baz.cpp");
  EXPECT_EQ(filteredFutRes4.globFiles[3], "dir2/foo.txt");
  EXPECT_EQ(filteredFutRes4.globFiles[4], "dir2/foo/README");

  EXPECT_EQ(filteredFutRes5.globFiles[0], "this");
  EXPECT_EQ(filteredFutRes5.globFiles[1], "this/filter");
  EXPECT_EQ(filteredFutRes5.globFiles[2], "this/filter/is");
  EXPECT_EQ(filteredFutRes5.globFiles[3], "this/filter/is/very");
}

TEST_F(FakePrefixFilteredBackingStoreTest, testCompareSimilarTreeObjectsById) {
  // The code that this test is testing only works when the
  // getFilterCoverageForPath check is immediately ready. See:
  // https://fburl.com/code/0xze5u4c
  //
  // Therefore this test will not work if we are running in debug mode because
  // we set detail::kImmediateFutureAlwaysDefer in debug mode.
  if (detail::kImmediateFutureAlwaysDefer) {
    return;
  }

  // These two trees have different filters, but the filters evaluate to the
  // same filtering results. These two trees are also different objects
  // altogether (i.e. they have different underlying ObjectIDs).
  //
  // These two trees should resolve to different objects, but a previous bug in
  // comparison logic caused them to evaluate as identical.
  auto substringFilter = std::make_unique<FakePrefixFilter>();
  auto treeFOID =
      FilteredObjectId{RelativePath{"bar"}, "foooo", makeTestId("0000")};
  auto treeFOIDFilter = treeFOID.filter();
  auto similarFilter = treeFOIDFilter.subpiece(0, treeFOIDFilter.size() - 2);
  // Ensure the two filters have the same coverage
  EXPECT_EQ(
      substringFilter->getFilterCoverageForPath(treeFOID.path(), similarFilter)
          .get(),
      substringFilter->getFilterCoverageForPath(treeFOID.path(), treeFOIDFilter)
          .get());
  // Ensure that the two objects are not identical
  auto similarObject = makeTestId("e1e10");
  EXPECT_NE(
      wrappedStore_->compareObjectsById(similarObject, treeFOID.object()),
      ObjectComparison::Identical);
  auto similarFOID = FilteredObjectId{
      treeFOID.path(),
      similarFilter,
      similarObject,
  };

  // We expect a tree with the same filter coverage but different underlying
  // objects to not be identical.
  EXPECT_NE(
      filteredStore_->compareObjectsById(
          ObjectId{treeFOID.getValue()}, ObjectId{similarFOID.getValue()}),
      ObjectComparison::Identical);
}

const auto kTestTimeout = 10s;

TEST_F(SaplingFilteredBackingStoreTest, testMercurialFFI) {
  // Set up one commit with a root tree
  auto filterRelPath = RelativePath{"filter"};
  auto rootFuture1 = filteredStoreFFI_->getRootTree(
      RootId{FilteredBackingStore::createFilteredRootId(
          commit1.value(),
          fmt::format("{}:{}", filterRelPath.piece(), commit1.value()))},
      ObjectFetchContext::getNullContext());
  auto rootDirRes = std::move(rootFuture1).get(kTestTimeout);

  // Get the object IDs of all the trees/files from the root dir.
  auto [dir2Name, dir2Entry] = *rootDirRes.tree->find("dir2"_pc);
  auto [srcName, srcEntry] = *rootDirRes.tree->find("src"_pc);
  auto fooTxtFindRes = rootDirRes.tree->find("foo.txt"_pc);
  auto barTxtFindRes = rootDirRes.tree->find("bar.txt"_pc);
  auto fooFindRes = rootDirRes.tree->find("foo"_pc);
  auto filteredOutFindRes = rootDirRes.tree->find("filtered_out"_pc);

  // Get all the files from the trees from commit 1.
  auto dir2Future = filteredStoreFFI_->getTree(
      dir2Entry.getObjectId(), ObjectFetchContext::getNullContext());
  auto dir2Res = std::move(dir2Future).get(kTestTimeout).tree;
  auto readmeFindRes = dir2Res->find("README"_pc);
  auto srcFuture = filteredStoreFFI_->getTree(
      srcEntry.getObjectId(), ObjectFetchContext::getNullContext());
  auto srcRes = std::move(srcFuture).get(kTestTimeout).tree;
  auto helloFindRes = srcRes->find("hello.txt"_pc);

  // We expect these files to be filtered
  EXPECT_EQ(fooFindRes, rootDirRes.tree->cend());
  EXPECT_EQ(readmeFindRes, dir2Res->cend());
  EXPECT_EQ(filteredOutFindRes, rootDirRes.tree->cend());

  // We expect these files to be present
  EXPECT_NE(fooTxtFindRes, rootDirRes.tree->cend());
  EXPECT_NE(barTxtFindRes, rootDirRes.tree->cend());
  EXPECT_NE(helloFindRes, srcRes->cend());
}

TEST_F(SaplingFilteredBackingStoreTest, testMercurialFFINullFilter) {
  // Set up one commit with a root tree
  auto rootFuture1 = filteredStoreFFI_->getRootTree(
      RootId{
          FilteredBackingStore::createFilteredRootId(commit1.value(), "null")},
      ObjectFetchContext::getNullContext());

  auto rootDirRes = std::move(rootFuture1).get(kTestTimeout);

  // Get the object IDs of all the trees/files from the root dir.
  auto [dir2Name, dir2Entry] = *rootDirRes.tree->find("dir2"_pc);
  auto [srcName, srcEntry] = *rootDirRes.tree->find("src"_pc);
  auto fooTxtFindRes = rootDirRes.tree->find("foo.txt"_pc);
  auto barTxtFindRes = rootDirRes.tree->find("bar.txt"_pc);
  auto fooFindRes = rootDirRes.tree->find("foo"_pc);
  auto filteredOutFindRes = rootDirRes.tree->find("filtered_out"_pc);

  // Get all the files from the trees from commit 1.
  auto dir2Future = filteredStoreFFI_->getTree(
      dir2Entry.getObjectId(), ObjectFetchContext::getNullContext());
  auto dir2Res = std::move(dir2Future).get(kTestTimeout).tree;
  auto readmeFindRes = dir2Res->find("README"_pc);
  auto srcFuture = filteredStoreFFI_->getTree(
      srcEntry.getObjectId(), ObjectFetchContext::getNullContext());
  auto srcRes = std::move(srcFuture).get(kTestTimeout).tree;
  auto helloFindRes = srcRes->find("hello.txt"_pc);

  // We expect all files to be present
  EXPECT_NE(fooFindRes, rootDirRes.tree->cend());
  EXPECT_NE(readmeFindRes, dir2Res->cend());
  EXPECT_NE(filteredOutFindRes, rootDirRes.tree->cend());
  EXPECT_NE(fooTxtFindRes, rootDirRes.tree->cend());
  EXPECT_NE(barTxtFindRes, rootDirRes.tree->cend());
  EXPECT_NE(helloFindRes, srcRes->cend());
}

TEST_F(SaplingFilteredBackingStoreTest, testMercurialFFIInvalidFOID) {
  // Set up one commit with a root tree
  auto filterRelPath = RelativePath{"filter"};
  auto rootFuture1 = filteredStoreFFI_->getRootTree(
      RootId{FilteredBackingStore::createFilteredRootId(
          commit1.value(),
          fmt::format("{}:{}", filterRelPath.piece(), commit1.value()))},
      ObjectFetchContext::getNullContext());

  auto rootDirRes = std::move(rootFuture1).get(kTestTimeout);

  // Get the object IDs of all the trees/files from the root dir.
  auto [dir2Name, dir2Entry] = *rootDirRes.tree->find("dir2"_pc);
  auto [srcName, srcEntry] = *rootDirRes.tree->find("src"_pc);
  auto fooTxtFindRes = rootDirRes.tree->find("foo.txt"_pc);
  auto barTxtFindRes = rootDirRes.tree->find("bar.txt"_pc);
  auto fooFindRes = rootDirRes.tree->find("foo"_pc);
  auto filteredOutFindRes = rootDirRes.tree->find("filtered_out"_pc);

  // Get all the files from the trees from commit 1. We intentionally use the
  // wrapped ObjectId instead of the FilteredObjectId to test whether we handle
  // invalid FOIDs correctly.
  auto dir2OID =
      FilteredObjectId::fromObjectId(dir2Entry.getObjectId()).object();
  EXPECT_THROW_RE(
      filteredStoreFFI_->getTree(dir2OID, ObjectFetchContext::getNullContext()),
      std::invalid_argument,
      ".*Invalid FilteredObjectId type byte 1.*");

  auto src2OID =
      FilteredObjectId::fromObjectId(srcEntry.getObjectId()).object();
  EXPECT_THROW_RE(
      filteredStoreFFI_->getTree(src2OID, ObjectFetchContext::getNullContext()),
      std::invalid_argument,
      ".*Invalid FilteredObjectId type byte 1.*");

  // We still expect foo and filtered_out to be filtered.
  EXPECT_EQ(fooFindRes, rootDirRes.tree->cend());
  EXPECT_EQ(filteredOutFindRes, rootDirRes.tree->cend());

  // We expect these files to be present
  EXPECT_NE(fooTxtFindRes, rootDirRes.tree->cend());
  EXPECT_NE(barTxtFindRes, rootDirRes.tree->cend());
}
} // namespace facebook::eden
