/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/store/Diff.h"

#include <folly/test/TestUtils.h>
#include <gmock/gmock.h>
#include <gtest/gtest.h>

#include "eden/fs/store/MemoryLocalStore.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/store/ScmStatusDiffCallback.h"
#include "eden/fs/testharness/FakeBackingStore.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestUtil.h"

using namespace facebook::eden;
using namespace std::chrono_literals;
using folly::Future;
using folly::StringPiece;
using std::make_shared;
using std::make_unique;
using ::testing::Pair;
using ::testing::UnorderedElementsAre;

namespace facebook {
namespace eden {
inline void PrintTo(ScmFileStatus status, ::std::ostream* os) {
  switch (status) {
    case ScmFileStatus::ADDED:
      *os << "ADDED";
      return;
    case ScmFileStatus::MODIFIED:
      *os << "MODIFIED";
      return;
    case ScmFileStatus::REMOVED:
      *os << "REMOVED";
      return;
    case ScmFileStatus::IGNORED:
      *os << "IGNORED";
      return;
  }
  *os << "unknown status " << static_cast<unsigned int>(status);
}
} // namespace eden
} // namespace facebook

class DiffTest : public ::testing::Test {
 protected:
  void SetUp() override {
    localStore_ = make_shared<MemoryLocalStore>();
    backingStore_ = make_shared<FakeBackingStore>(localStore_);
    store_ = ObjectStore::create(
        localStore_, backingStore_, std::make_shared<EdenStats>());
  }

  Future<std::unique_ptr<ScmStatus>> diffCommits(
      StringPiece commit1,
      StringPiece commit2) {
    return diffCommitsForStatus(
        store_.get(), makeTestHash(commit1), makeTestHash(commit2));
  }

  std::shared_ptr<LocalStore> localStore_;
  std::shared_ptr<FakeBackingStore> backingStore_;
  std::shared_ptr<ObjectStore> store_;
};

TEST_F(DiffTest, unknownCommit) {
  auto future = diffCommits("1", "1");
  EXPECT_THROW_RE(
      std::move(future).get(100ms), std::domain_error, "commit .* not found");
}

TEST_F(DiffTest, sameCommit) {
  FakeTreeBuilder builder;

  builder.setFile("a/b/c/d/e/f.txt", "contents");
  builder.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("1", builder)->setReady();

  auto result = diffCommits("1", "1").get(100ms);
  EXPECT_THAT(result->errors, UnorderedElementsAre());
  EXPECT_THAT(result->entries, UnorderedElementsAre());
}

TEST_F(DiffTest, basicDiff) {
  FakeTreeBuilder builder;

  builder.setFile("a/b/c/d/e/f.txt", "contents");
  builder.setFile("a/b/1.txt", "1");
  builder.setFile("a/b/2.txt", "2");
  builder.setFile("a/b/3.txt", "3");
  builder.setFile("src/main.c", "hello world");
  builder.setFile("src/lib.c", "helper code");
  builder.setFile("src/test/test.c", "testing");
  builder.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("1", builder)->setReady();

  // Modify one file, add one file, and remove one file
  auto builder2 = builder.clone();
  builder2.replaceFile("src/main.c", "hello world v2");
  builder2.setFile("src/test/test2.c", "another test");
  builder2.removeFile("a/b/1.txt");
  builder2.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("2", builder2)->setReady();

  auto result = diffCommits("1", "2").get(100ms);
  EXPECT_THAT(result->errors, UnorderedElementsAre());
  EXPECT_THAT(
      result->entries,
      UnorderedElementsAre(
          Pair("src/main.c", ScmFileStatus::MODIFIED),
          Pair("src/test/test2.c", ScmFileStatus::ADDED),
          Pair("a/b/1.txt", ScmFileStatus::REMOVED)));
}

TEST_F(DiffTest, directoryOrdering) {
  FakeTreeBuilder builder;

  // Test adding and removing files at the beginning and end of the sorted
  // directory list.  This exercises different code paths in the diff logic.
  builder.setFile("src/foo/bbb.txt", "b");
  builder.setFile("src/foo/ccc.txt", "c");
  builder.setFile("src/foo/xxx.txt", "x");
  builder.setFile("src/foo/yyy.txt", "y");
  builder.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("1", builder)->setReady();

  auto builder2 = builder.clone();
  builder2.setFile("src/foo/aaa.txt", "a");
  builder2.setFile("src/foo/zzz.txt", "z");
  builder2.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("2", builder2)->setReady();

  auto result = diffCommits("1", "2").get(100ms);
  EXPECT_THAT(result->errors, UnorderedElementsAre());
  EXPECT_THAT(
      result->entries,
      UnorderedElementsAre(
          Pair("src/foo/aaa.txt", ScmFileStatus::ADDED),
          Pair("src/foo/zzz.txt", ScmFileStatus::ADDED)));

  auto result2 = diffCommits("2", "1").get(100ms);
  EXPECT_THAT(result2->errors, UnorderedElementsAre());
  EXPECT_THAT(
      result2->entries,
      UnorderedElementsAre(
          Pair("src/foo/aaa.txt", ScmFileStatus::REMOVED),
          Pair("src/foo/zzz.txt", ScmFileStatus::REMOVED)));
}

TEST_F(DiffTest, modeChange) {
  FakeTreeBuilder builder;

  builder.setFile("some_file", "contents");
  builder.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("1", builder)->setReady();

  // Modify one file, add one file, and remove one file
  auto builder2 = builder.clone();
  builder2.replaceSymlink("some_file", "contents");
  builder2.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("2", builder2)->setReady();

  auto result = diffCommits("1", "2").get(100ms);
  EXPECT_THAT(result->errors, UnorderedElementsAre());
  EXPECT_THAT(
      result->entries,
      UnorderedElementsAre(Pair("some_file", ScmFileStatus::MODIFIED)));

  auto result2 = diffCommits("2", "1").get(100ms);
  EXPECT_THAT(result2->errors, UnorderedElementsAre());
  EXPECT_THAT(
      result2->entries,
      UnorderedElementsAre(Pair("some_file", ScmFileStatus::MODIFIED)));
}

TEST_F(DiffTest, newDirectory) {
  FakeTreeBuilder builder;

  builder.setFile("src/foo/a.txt", "a");
  builder.setFile("src/foo/b.txt", "b");
  builder.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("1", builder)->setReady();

  auto builder2 = builder.clone();
  builder2.setFile("src/foo/a/b/c.txt", "c");
  builder2.setFile("src/foo/a/b/d.txt", "d");
  builder2.setFile("src/foo/a/b/e.txt", "e");
  builder2.setFile("src/foo/a/b/f/g.txt", "g");
  builder2.setFile("src/foo/z/y/x.txt", "x");
  builder2.setFile("src/foo/z/y/w.txt", "w");
  builder2.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("2", builder2)->setReady();

  auto result = diffCommits("1", "2").get(100ms);
  EXPECT_THAT(result->errors, UnorderedElementsAre());
  auto expectedResults = UnorderedElementsAre(
      Pair("src/foo/a/b/c.txt", ScmFileStatus::ADDED),
      Pair("src/foo/a/b/d.txt", ScmFileStatus::ADDED),
      Pair("src/foo/a/b/e.txt", ScmFileStatus::ADDED),
      Pair("src/foo/a/b/f/g.txt", ScmFileStatus::ADDED),
      Pair("src/foo/z/y/x.txt", ScmFileStatus::ADDED),
      Pair("src/foo/z/y/w.txt", ScmFileStatus::ADDED));
  EXPECT_THAT(result->entries, expectedResults);

  auto result2 = diffCommits("2", "1").get(100ms);
  EXPECT_THAT(result2->errors, UnorderedElementsAre());
  EXPECT_THAT(
      result2->entries,
      UnorderedElementsAre(
          Pair("src/foo/a/b/c.txt", ScmFileStatus::REMOVED),
          Pair("src/foo/a/b/d.txt", ScmFileStatus::REMOVED),
          Pair("src/foo/a/b/e.txt", ScmFileStatus::REMOVED),
          Pair("src/foo/a/b/f/g.txt", ScmFileStatus::REMOVED),
          Pair("src/foo/z/y/x.txt", ScmFileStatus::REMOVED),
          Pair("src/foo/z/y/w.txt", ScmFileStatus::REMOVED)));

  // Test calling diffTrees() with hashes
  auto callback = std::make_unique<ScmStatusDiffCallback>();
  auto callbackPtr = callback.get();

  auto treeResult = diffTrees(
                        store_.get(),
                        callbackPtr,
                        builder.getRoot()->get().getHash(),
                        builder2.getRoot()->get().getHash())
                        .thenValue([callback = std::move(callback)](auto&&) {
                          return callback->extractStatus();
                        })
                        .get(100ms);
  EXPECT_THAT(treeResult.errors, UnorderedElementsAre());
  EXPECT_THAT(treeResult.entries, expectedResults);

  // Test calling diffTrees() with Tree objects
  auto callback2 = std::make_unique<ScmStatusDiffCallback>();
  auto callbackPtr2 = callback2.get();

  auto treeResult2 = diffTrees(
                         store_.get(),
                         callbackPtr2,
                         builder.getRoot()->get(),
                         builder2.getRoot()->get())
                         .thenValue([callback2 = std::move(callback2)](auto&&) {
                           return callback2->extractStatus();
                         })
                         .get(100ms);
  EXPECT_THAT(treeResult2.errors, UnorderedElementsAre());
  EXPECT_THAT(treeResult2.entries, expectedResults);
}

TEST_F(DiffTest, fileToDirectory) {
  FakeTreeBuilder builder;

  builder.setFile("src/foo/a.txt", "a");
  builder.setFile("src/foo/b.txt", "b", /* executable */ true);
  builder.setFile("src/foo/a", "regular file");
  builder.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("1", builder)->setReady();

  auto builder2 = builder.clone();
  builder2.removeFile("src/foo/a");
  builder2.setFile("src/foo/a/b/c.txt", "c");
  builder2.setFile("src/foo/a/b/d.txt", "d");
  builder2.setFile("src/foo/a/b/e.txt", "e");
  builder2.setFile("src/foo/a/b/f/g.txt", "g");
  builder2.setFile("src/foo/z/y/x.txt", "x");
  builder2.setFile("src/foo/z/y/w.txt", "w");
  builder2.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("2", builder2)->setReady();

  auto result = diffCommits("1", "2").get(100ms);
  EXPECT_THAT(result->errors, UnorderedElementsAre());
  EXPECT_THAT(
      result->entries,
      UnorderedElementsAre(
          Pair("src/foo/a", ScmFileStatus::REMOVED),
          Pair("src/foo/a/b/c.txt", ScmFileStatus::ADDED),
          Pair("src/foo/a/b/d.txt", ScmFileStatus::ADDED),
          Pair("src/foo/a/b/e.txt", ScmFileStatus::ADDED),
          Pair("src/foo/a/b/f/g.txt", ScmFileStatus::ADDED),
          Pair("src/foo/z/y/x.txt", ScmFileStatus::ADDED),
          Pair("src/foo/z/y/w.txt", ScmFileStatus::ADDED)));

  auto result2 = diffCommits("2", "1").get(100ms);
  EXPECT_THAT(result2->errors, UnorderedElementsAre());
  EXPECT_THAT(
      result2->entries,
      UnorderedElementsAre(
          Pair("src/foo/a", ScmFileStatus::ADDED),
          Pair("src/foo/a/b/c.txt", ScmFileStatus::REMOVED),
          Pair("src/foo/a/b/d.txt", ScmFileStatus::REMOVED),
          Pair("src/foo/a/b/e.txt", ScmFileStatus::REMOVED),
          Pair("src/foo/a/b/f/g.txt", ScmFileStatus::REMOVED),
          Pair("src/foo/z/y/x.txt", ScmFileStatus::REMOVED),
          Pair("src/foo/z/y/w.txt", ScmFileStatus::REMOVED)));
}

TEST_F(DiffTest, blockedFutures) {
  FakeTreeBuilder builder;

  // Build the commits, but do not make the data ready yet in the
  // FakeBackingStore, so that Futures needing this data will not complete
  // immediately.

  // Create data for the first commit
  builder.setFile("a/b/c/d/e/f.txt", "contents");
  builder.setFile("a/b/1.txt", "1");
  builder.setFile("a/b/2.txt", "2");
  builder.setFile("a/b/3.txt", "3");
  builder.setFile("src/main.c", "hello world");
  builder.setFile("src/lib.c", "helper code");
  builder.setFile("src/test/test.c", "testing");
  builder.finalize(backingStore_, /* setReady */ false);
  auto root1 = backingStore_->putCommit("1", builder);

  // Create data for the second commit
  auto builder2 = builder.clone();
  builder2.replaceFile("src/main.c", "hello world v2");
  builder2.setFile("src/test/test2.c", "another test");
  builder2.removeFile("a/b/c/d/e/f.txt");
  builder2.replaceFile("a/b/1.txt", "1", /* executable */ true);
  builder2.setFile("src/newdir/a.txt", "a");
  builder2.setFile("src/newdir/b/c.txt", "c");
  builder2.setFile("src/newdir/b/d.txt", "d");
  builder2.finalize(backingStore_, /* setReady */ false);
  auto root2 = backingStore_->putCommit("2", builder2);

  auto resultFuture = diffCommits("1", "2");
  EXPECT_FALSE(resultFuture.isReady());

  // Now gradually mark the data in each commit ready, so the diff
  // will make progress as we mark more things ready.

  // Make the root commit & tree for commit 1
  root1->setReady();
  builder.setReady("");
  EXPECT_FALSE(resultFuture.isReady());

  // Mark everything under src/ ready in both trees
  builder.setAllReadyUnderTree("src");
  builder2.setAllReadyUnderTree("src");
  EXPECT_FALSE(resultFuture.isReady());

  // Mark the root commit and tree ready for commit 2.
  root2->setReady();
  builder2.setReady("");
  EXPECT_FALSE(resultFuture.isReady());

  // Mark the hierarchy under "a" ready.
  // Note that we don't have to mark blobs ready, the diffing code
  // only needs to get the tree data.
  builder.setReady("a");
  builder2.setReady("a");
  EXPECT_FALSE(resultFuture.isReady());
  builder.setReady("a/b");
  builder2.setReady("a/b");
  EXPECT_FALSE(resultFuture.isReady());
  builder.setReady("a/b/c");
  EXPECT_FALSE(resultFuture.isReady());
  builder.setReady("a/b/c/d");
  EXPECT_FALSE(resultFuture.isReady());
  // a/b/c/d/e is the last directory that remains not ready yet.
  // Once we mark it ready the diff operation should complete.
  builder.setReady("a/b/c/d/e");
  EXPECT_TRUE(resultFuture.isReady());

  builder.setAllReady();
  builder2.setAllReady();
  ASSERT_TRUE(resultFuture.isReady());

  auto result = std::move(resultFuture).get();
  EXPECT_THAT(result->errors, UnorderedElementsAre());
  EXPECT_THAT(
      result->entries,
      UnorderedElementsAre(
          Pair("src/main.c", ScmFileStatus::MODIFIED),
          Pair("src/test/test2.c", ScmFileStatus::ADDED),
          Pair("a/b/c/d/e/f.txt", ScmFileStatus::REMOVED),
          Pair("a/b/1.txt", ScmFileStatus::MODIFIED),
          Pair("src/newdir/a.txt", ScmFileStatus::ADDED),
          Pair("src/newdir/b/c.txt", ScmFileStatus::ADDED),
          Pair("src/newdir/b/d.txt", ScmFileStatus::ADDED)));
}

TEST_F(DiffTest, loadTreeError) {
  FakeTreeBuilder builder;

  // Create data for the first commit
  builder.setFile("a/b/1.txt", "1");
  builder.setFile("a/b/2.txt", "2");
  builder.setFile("a/b/3.txt", "3");
  builder.setFile("x/y/test.txt", "test");
  builder.setFile("x/y/z/file1.txt", "file1");
  builder.finalize(backingStore_, /* setReady */ false);
  auto root1 = backingStore_->putCommit("1", builder);

  // Create data for the second commit
  auto builder2 = builder.clone();
  builder2.replaceFile("a/b/3.txt", "new3");
  builder2.setFile("x/y/z/file2.txt", "file2");
  builder2.finalize(backingStore_, /* setReady */ false);
  auto root2 = backingStore_->putCommit("2", builder2);

  auto resultFuture = diffCommits("1", "2");
  EXPECT_FALSE(resultFuture.isReady());

  // Make the root commit & tree for commit 1
  root1->setReady();
  builder.setReady("");
  root2->setReady();
  builder2.setReady("");
  EXPECT_FALSE(resultFuture.isReady());

  builder.setReady("x");
  builder.setReady("x/y");
  builder.setReady("x/y/z");

  builder2.setReady("x");
  builder2.setReady("x/y");
  // Report an error loading x/y/z on commit2
  builder2.triggerError("x/y/z", std::runtime_error("oh noes"));
  EXPECT_FALSE(resultFuture.isReady());

  builder.setAllReadyUnderTree("a");
  builder2.setAllReadyUnderTree("a");
  ASSERT_TRUE(resultFuture.isReady());

  auto result = std::move(resultFuture).get();
  EXPECT_THAT(
      result->errors,
      UnorderedElementsAre(Pair(
          "x/y/z",
          folly::exceptionStr(std::runtime_error("oh noes")).c_str())));
  EXPECT_THAT(
      result->entries,
      UnorderedElementsAre(Pair("a/b/3.txt", ScmFileStatus::MODIFIED)));
}
