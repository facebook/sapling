/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/ExceptionWrapper.h>
#include <folly/logging/xlog.h>
#include <folly/test/TestUtils.h>
#include <gmock/gmock.h>
#include <gtest/gtest.h>

#include "eden/fs/config/CheckoutConfig.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/model/git/TopLevelIgnores.h"
#include "eden/fs/store/DiffContext.h"
#include "eden/fs/store/ScmStatusDiffCallback.h"
#include "eden/fs/testharness/FakeBackingStore.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/StoredObject.h"
#include "eden/fs/testharness/TestChecks.h"
#include "eden/fs/testharness/TestMount.h"
#include "eden/fs/testharness/TestUtil.h"

using namespace facebook::eden;
using namespace std::chrono_literals;
using std::string;
using ::testing::UnorderedElementsAre;
using ::testing::UnorderedElementsAreArray;

template <typename T>
T getFutureResult(folly::Future<T>& future, const char* filename, int line) {
  if (!future.isReady()) {
    ADD_FAILURE_AT(filename, line) << "future not ready";
    throw folly::FutureTimeout();
  }
  if (future.hasException()) {
    ADD_FAILURE_AT(filename, line) << "future failed";
    // fall through and let get() throw.
  }
  return std::move(future).get();
}

#define EXPECT_FUTURE_RESULT(future) getFutureResult(future, __FILE__, __LINE__)

constexpr folly::StringPiece kReadmeContent{"No one reads docs.\n"};

/**
 * A helper class for implementing the various diff tests.
 *
 * This is not implemented as a gtest fixture because using a standalone class
 * allows us to use multiple separate DiffTest objects in the same test case.
 * (This is mostly for convenience.  We could split things up into more test
 * cases if necessary, but defining so many separate TEST functions becomes
 * awkward.)
 */
class DiffTest {
 public:
  DiffTest() {
    // Set up a directory structure that we will use for most
    // of the tests below
    builder_.setFiles({
        {"src/1.txt", "This is src/1.txt.\n"},
        {"src/2.txt", "This is src/2.txt.\n"},
        {"src/a/b/3.txt", "This is 3.txt.\n"},
        {"src/a/b/c/4.txt", "This is 4.txt.\n"},
        {"doc/readme.txt", kReadmeContent},
        {"toplevel.txt", "toplevel\n"},
    });
    mount_.initialize(builder_);
  }

  explicit DiffTest(
      std::initializer_list<FakeTreeBuilder::FileInfo>&& fileArgs) {
    builder_.setFiles(std::move(fileArgs));
    mount_.initialize(builder_);
  }

  ScmStatus diff(
      bool listIgnored = false,
      folly::StringPiece systemWideIgnoreFileContents = "",
      folly::StringPiece userIgnoreFileContents = "",
      CaseSensitivity caseSensitive = kPathMapDefaultCaseSensitive) {
    ScmStatusDiffCallback callback;
    DiffContext diffContext{
        &callback,
        folly::CancellationToken{},
        ObjectFetchContext::getNullContext(),
        listIgnored,
        caseSensitive,
        true,
        mount_.getEdenMount()->getObjectStore(),
        std::make_unique<TopLevelIgnores>(
            systemWideIgnoreFileContents, userIgnoreFileContents)};
    auto commitId = mount_.getEdenMount()->getCheckedOutRootId();
    auto diffFuture = mount_.getEdenMount()
                          ->diff(mount_.getRootInode(), &diffContext, commitId)
                          .semi()
                          .via(mount_.getServerExecutor().get());
    mount_.drainServerExecutor();
    EXPECT_FUTURE_RESULT(diffFuture);
    return callback.extractStatus();
  }
  ImmediateFuture<ScmStatus> diffFuture(bool listIgnored = false) {
    auto commitId = mount_.getEdenMount()->getWorkingCopyParent();
    auto diffFuture = mount_.getEdenMount()->diff(
        mount_.getRootInode(),
        commitId,
        folly::CancellationToken{},
        ObjectFetchContext::getNullContext(),
        listIgnored,
        /*enforceCurrentParent=*/false);
    return std::move(diffFuture)
        .thenValue([](std::unique_ptr<ScmStatus>&& result) { return *result; });
  }

  ScmStatus resetCommitAndDiff(FakeTreeBuilder& builder, bool loadInodes);

  void checkNoChanges() {
    auto result = diff();
    EXPECT_THAT(*result.entries(), UnorderedElementsAre());
  }

  void testResetFileModified(bool loadInodes);

  FakeTreeBuilder& getBuilder() {
    return builder_;
  }
  TestMount& getMount() {
    return mount_;
  }

 private:
  FakeTreeBuilder builder_;
  TestMount mount_;
  std::string readmeContent_;
};

/**
 * This method performs several steps:
 *
 * - Finalizes the supplied FakeTreeBuilder
 * - Creates a new commit from the resulting tree
 * - Calls EdenMount::resetCommit() to reset the current snapshot to point to
 *   this commit.  (This leaves the working directory unchanged, and only
 *   updates the current commit ID.)
 * - Calls EdenMount::diff(), waits for it to complete, and returns the
 *   results.
 */
ScmStatus DiffTest::resetCommitAndDiff(
    FakeTreeBuilder& builder,
    bool loadInodes) {
  if (loadInodes) {
    mount_.loadAllInodes();
  }
  mount_.resetCommit(builder, /* setReady = */ true);
  auto df = diffFuture().semi().via(mount_.getServerExecutor().get());
  mount_.drainServerExecutor();
  return EXPECT_FUTURE_RESULT(df);
}

TEST(DiffTest, noChanges) {
  DiffTest test;
  // Run diff with no inodes loaded
  test.checkNoChanges();

  // Load all inodes then re-run the diff
  test.getMount().loadAllInodes();
  test.checkNoChanges();

  // Write the original contents to a file, and make sure it
  // still does not show up as changed.
  test.getMount().overwriteFile("src/1.txt", "This is src/1.txt.\n");
  test.checkNoChanges();
}

TEST(DiffTest, fileModified) {
  DiffTest test;
  test.getMount().overwriteFile("src/1.txt", "This file has been updated.\n");

  auto result = test.diff();
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("src/1.txt", ScmFileStatus::MODIFIED)));
}

#ifndef _WIN32
TEST(DiffTest, fileModeChanged) {
  DiffTest test;
  test.getMount().chmod("src/2.txt", 0755);

  auto result = test.diff();
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("src/2.txt", ScmFileStatus::MODIFIED)));
}

#endif // !_WIN32

TEST(DiffTest, fileRemoved) {
  DiffTest test;
  test.getMount().deleteFile("src/1.txt");

  auto result = test.diff();
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("src/1.txt", ScmFileStatus::REMOVED)));
}

TEST(DiffTest, fileAdded) {
  DiffTest test;
  test.getMount().addFile("src/new.txt", "extra stuff");

  auto result = test.diff();
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("src/new.txt", ScmFileStatus::ADDED)));
}

TEST(DiffTest, directoryRemoved) {
  DiffTest test;
  auto& mount = test.getMount();
  mount.deleteFile("src/a/b/3.txt");
  mount.deleteFile("src/a/b/c/4.txt");
  mount.rmdir("src/a/b/c");
  mount.rmdir("src/a/b");

  auto result = test.diff();
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("src/a/b/c/4.txt", ScmFileStatus::REMOVED),
          std::make_pair("src/a/b/3.txt", ScmFileStatus::REMOVED)));
}

TEST(DiffTest, directoryAdded) {
  DiffTest test;
  auto& mount = test.getMount();
  mount.mkdir("src/new");
  mount.mkdir("src/new/subdir");
  mount.addFile("src/new/file.txt", "extra stuff");
  mount.addFile("src/new/subdir/foo.txt", "extra stuff");
  mount.addFile("src/new/subdir/bar.txt", "more extra stuff");

  auto result = test.diff();
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("src/new/file.txt", ScmFileStatus::ADDED),
          std::make_pair("src/new/subdir/foo.txt", ScmFileStatus::ADDED),
          std::make_pair("src/new/subdir/bar.txt", ScmFileStatus::ADDED)));
}

TEST(DiffTest, dirReplacedWithFile) {
  DiffTest test;
  auto& mount = test.getMount();
  mount.deleteFile("src/a/b/3.txt");
  mount.deleteFile("src/a/b/c/4.txt");
  mount.rmdir("src/a/b/c");
  mount.rmdir("src/a/b");
  mount.addFile("src/a/b", "this is now a file");

  auto result = test.diff();
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("src/a/b", ScmFileStatus::ADDED),
          std::make_pair("src/a/b/3.txt", ScmFileStatus::REMOVED),
          std::make_pair("src/a/b/c/4.txt", ScmFileStatus::REMOVED)));
}

TEST(DiffTest, fileReplacedWithDir) {
  DiffTest test;
  auto& mount = test.getMount();
  mount.deleteFile("src/2.txt");
  mount.mkdir("src/2.txt");
  mount.mkdir("src/2.txt/subdir");
  mount.addFile("src/2.txt/file.txt", "extra stuff");
  mount.addFile("src/2.txt/subdir/foo.txt", "extra stuff");
  mount.addFile("src/2.txt/subdir/bar.txt", "more extra stuff");

  auto result = test.diff();
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("src/2.txt/file.txt", ScmFileStatus::ADDED),
          std::make_pair("src/2.txt/subdir/foo.txt", ScmFileStatus::ADDED),
          std::make_pair("src/2.txt/subdir/bar.txt", ScmFileStatus::ADDED),
          std::make_pair("src/2.txt", ScmFileStatus::REMOVED)));
}

TEST(DiffTest, fileCasingChange) {
  DiffTest test;
  auto& mount = test.getMount();
  mount.deleteFile("doc/readme.txt");
  mount.rmdir("doc");

  mount.mkdir("DOC");
  mount.addFile("DOC/README.txt", kReadmeContent);

  auto resultInsensitive =
      test.diff(false, "", "", CaseSensitivity::Insensitive);
  EXPECT_THAT(*resultInsensitive.entries(), UnorderedElementsAre());

  auto resultSensitive = test.diff(false, "", "", CaseSensitivity::Sensitive);
  EXPECT_THAT(
      *resultSensitive.entries(),
      UnorderedElementsAre(
          std::make_pair("doc/readme.txt", ScmFileStatus::REMOVED),
          std::make_pair("DOC/README.txt", ScmFileStatus::ADDED)));
}

// Test file adds/removes/modifications with various orderings of names between
// the TreeInode entries and Tree entries.  This exercises the code that walks
// through the two entry lists comparing entry names.
TEST(DiffTest, pathOrdering) {
  DiffTest test({
      {"one/bbb.txt", "test\n"},
      {"one/xxx.txt", "test\n"},
      {"two/aaa.txt", "test\n"},
      {"two/bbb.txt", "test\n"},
      {"two/mmm.txt", "test\n"},
      {"two/xxx.txt", "test\n"},
      {"two/zzz.txt", "test\n"},
      {"three/aaa.txt", "test\n"},
      {"three/bbb.txt", "test\n"},
      {"three/mmm.txt", "test\n"},
      {"three/xxx.txt", "test\n"},
      {"three/zzz.txt", "test\n"},
  });
  auto& mount = test.getMount();

  // In directory one:
  // Add a file so that the TreeInode has the first entry, with no
  // corresponding entry in the source control tree.
  mount.addFile("one/aaa.txt", "test");
  // Add a file in the middle of the two entries in the source control Tree
  mount.addFile("one/mmm.txt", "test");
  // Add a file so that the TreeInode has the last entry, with no
  // corresponding entry in the source control tree.
  mount.addFile("one/zzz.txt", "test");

  // In directory two, remove the opposite entries, so that the source control
  // Tree has the first and last entries.
  mount.deleteFile("two/aaa.txt");
  mount.deleteFile("two/mmm.txt");
  mount.deleteFile("two/zzz.txt");

  // In directory three, overwrite these 3 entries, so that the first and last
  // files are modified, plus one in the middle.
  mount.overwriteFile("three/aaa.txt", "updated contents\n");
  mount.overwriteFile("three/mmm.txt", "updated contents\n");
  mount.overwriteFile("three/zzz.txt", "updated contents\n");

  // Perform the diff
  auto result = test.diff();
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("one/aaa.txt", ScmFileStatus::ADDED),
          std::make_pair("one/mmm.txt", ScmFileStatus::ADDED),
          std::make_pair("one/zzz.txt", ScmFileStatus::ADDED),
          std::make_pair("two/aaa.txt", ScmFileStatus::REMOVED),
          std::make_pair("two/mmm.txt", ScmFileStatus::REMOVED),
          std::make_pair("two/zzz.txt", ScmFileStatus::REMOVED),
          std::make_pair("three/aaa.txt", ScmFileStatus::MODIFIED),
          std::make_pair("three/mmm.txt", ScmFileStatus::MODIFIED),
          std::make_pair("three/zzz.txt", ScmFileStatus::MODIFIED)));
}

/*
 * The following tests modify the directory contents using resetCommit()
 * This exercises a different code path than when using FUSE-like filesystem
 * APIs.  When using the normal filesystem APIs we end up with materialized
 * files.  When using resetCommit() we end up with files that are not
 * materialized, but are nonetheless different than the current commit.
 */

void testResetFileModified(bool loadInodes) {
  SCOPED_TRACE(folly::to<string>("loadInodes=", loadInodes));

  DiffTest t;
  auto b2 = t.getBuilder().clone();
  b2.replaceFile("src/1.txt", "This file has been updated.\n");

  auto result = t.resetCommitAndDiff(b2, loadInodes);
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("src/1.txt", ScmFileStatus::MODIFIED)));
}

TEST(DiffTest, resetFileModified) {
  testResetFileModified(true);
  testResetFileModified(false);
}

// TODO: T66590035
#ifndef _WIN32
void testResetFileModeChanged(bool loadInodes) {
  SCOPED_TRACE(folly::to<string>("loadInodes=", loadInodes));

  DiffTest t;
  auto b2 = t.getBuilder().clone();
  b2.replaceFile("src/1.txt", "This is src/1.txt.\n", true);

  auto result = t.resetCommitAndDiff(b2, loadInodes);
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("src/1.txt", ScmFileStatus::MODIFIED)));
}

TEST(DiffTest, resetFileModeChanged) {
  testResetFileModeChanged(true);
  testResetFileModeChanged(false);
}
#endif

void testResetFileRemoved(bool loadInodes) {
  SCOPED_TRACE(folly::to<string>("loadInodes=", loadInodes));

  DiffTest t;
  // Create a commit with a new file added.
  // When we reset to it (without changing the working directory) it will look
  // like we have removed this file.
  auto b2 = t.getBuilder().clone();
  b2.setFile("src/notpresent.txt", "never present in the working directory");

  auto result = t.resetCommitAndDiff(b2, loadInodes);
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("src/notpresent.txt", ScmFileStatus::REMOVED)));
}

TEST(DiffTest, resetFileRemoved) {
  testResetFileRemoved(true);
  testResetFileRemoved(false);
}

void testResetFileAdded(bool loadInodes) {
  SCOPED_TRACE(folly::to<string>("loadInodes=", loadInodes));

  DiffTest t;
  // Create a commit with a file removed.
  // When we reset to it (without changing the working directory) it will look
  // like we have added this file.
  auto b2 = t.getBuilder().clone();
  b2.removeFile("src/1.txt");

  auto result = t.resetCommitAndDiff(b2, loadInodes);
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(std::make_pair("src/1.txt", ScmFileStatus::ADDED)));
}

TEST(DiffTest, resetFileAdded) {
  testResetFileAdded(true);
  testResetFileAdded(false);
}

void testResetDirectoryRemoved(bool loadInodes) {
  SCOPED_TRACE(folly::to<string>("loadInodes=", loadInodes));

  DiffTest t;
  // Create a commit with a new directory added.
  // When we reset to it (without changing the working directory) it will look
  // like we have removed this directory.
  auto b2 = t.getBuilder().clone();
  b2.setFile("src/extradir/foo.txt", "foo");
  b2.setFile("src/extradir/bar.txt", "bar");
  b2.setFile("src/extradir/sub/1.txt", "1");
  b2.setFile("src/extradir/sub/xyz.txt", "xyz");
  b2.setFile("src/extradir/a/b/c/d/e.txt", "test");

  auto result = t.resetCommitAndDiff(b2, loadInodes);
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("src/extradir/foo.txt", ScmFileStatus::REMOVED),
          std::make_pair("src/extradir/bar.txt", ScmFileStatus::REMOVED),
          std::make_pair("src/extradir/sub/1.txt", ScmFileStatus::REMOVED),
          std::make_pair("src/extradir/sub/xyz.txt", ScmFileStatus::REMOVED),
          std::make_pair(
              "src/extradir/a/b/c/d/e.txt", ScmFileStatus::REMOVED)));
}

TEST(DiffTest, resetDirectoryRemoved) {
  testResetDirectoryRemoved(true);
  testResetDirectoryRemoved(false);
}

void testResetDirectoryAdded(bool loadInodes) {
  SCOPED_TRACE(folly::to<string>("loadInodes=", loadInodes));

  DiffTest t;
  // Create a commit with a directory removed.
  // When we reset to it (without changing the working directory) it will look
  // like we have added this directory.
  auto b2 = t.getBuilder().clone();
  b2.removeFile("src/a/b/3.txt");
  b2.removeFile("src/a/b/c/4.txt");

  auto result = t.resetCommitAndDiff(b2, loadInodes);
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("src/a/b/3.txt", ScmFileStatus::ADDED),
          std::make_pair("src/a/b/c/4.txt", ScmFileStatus::ADDED)));
}

TEST(DiffTest, resetDirectoryAdded) {
  testResetDirectoryAdded(true);
  testResetDirectoryAdded(false);
}

void testResetReplaceDirWithFile(bool loadInodes) {
  SCOPED_TRACE(folly::to<string>("loadInodes=", loadInodes));

  DiffTest t;
  // Create a commit with 2.txt replaced by a directory added.
  // When we reset to it (without changing the working directory) it will look
  // like we have replaced this directory with the 2.txt file.
  auto b2 = t.getBuilder().clone();
  b2.removeFile("src/2.txt");
  b2.setFile("src/2.txt/foo.txt", "foo");
  b2.setFile("src/2.txt/bar.txt", "bar");
  b2.setFile("src/2.txt/sub/1.txt", "1");
  b2.setFile("src/2.txt/sub/xyz.txt", "xyz");
  b2.setFile("src/2.txt/a/b/c/d/e.txt", "test");

  auto result = t.resetCommitAndDiff(b2, loadInodes);
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("src/2.txt", ScmFileStatus::ADDED),
          std::make_pair("src/2.txt/foo.txt", ScmFileStatus::REMOVED),
          std::make_pair("src/2.txt/bar.txt", ScmFileStatus::REMOVED),
          std::make_pair("src/2.txt/sub/1.txt", ScmFileStatus::REMOVED),
          std::make_pair("src/2.txt/sub/xyz.txt", ScmFileStatus::REMOVED),
          std::make_pair("src/2.txt/a/b/c/d/e.txt", ScmFileStatus::REMOVED)));
}

TEST(DiffTest, resetReplaceDirWithFile) {
  testResetReplaceDirWithFile(true);
  testResetReplaceDirWithFile(false);
}

void testResetReplaceFileWithDir(bool loadInodes) {
  SCOPED_TRACE(folly::to<string>("loadInodes=", loadInodes));

  DiffTest t;
  // Create a commit with a directory removed and replaced with a file.
  // When we reset to it (without changing the working directory) it will look
  // like we have removed the file and replaced it with the directory.
  auto b2 = t.getBuilder().clone();
  b2.removeFile("src/a/b/3.txt");
  b2.removeFile("src/a/b/c/4.txt");
  b2.setFile("src/a", "a is now a file");

  auto result = t.resetCommitAndDiff(b2, loadInodes);
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("src/a/b/3.txt", ScmFileStatus::ADDED),
          std::make_pair("src/a/b/c/4.txt", ScmFileStatus::ADDED),
          std::make_pair("src/a", ScmFileStatus::REMOVED)));
}

TEST(DiffTest, resetReplaceFileWithDir) {
  testResetReplaceFileWithDir(true);
  testResetReplaceFileWithDir(false);
}

// Test with a .gitignore file in the top-level directory
TEST(DiffTest, ignoreToplevelOnly) {
  DiffTest test({
      {".gitignore", "/1.txt\nignore.txt\njunk/\n!important.txt\n"},
      {"a/b.txt", "test\n"},
      {"src/x.txt", "test\n"},
      {"src/y.txt", "test\n"},
      {"src/z.txt", "test\n"},
      {"src/foo/bar.txt", "test\n"},
  });

  // Add some untracked files, some of which match the ignore patterns
  test.getMount().addFile("1.txt", "new\n");
  test.getMount().addFile("ignore.txt", "new\n");
  test.getMount().addFile("src/1.txt", "new\n");
  test.getMount().addFile("src/foo/ignore.txt", "new\n");
  test.getMount().mkdir("src/foo/abc");
  test.getMount().mkdir("src/foo/abc/xyz");
  test.getMount().addFile("src/foo/abc/xyz/ignore.txt", "new\n");
  test.getMount().mkdir("junk");
  test.getMount().addFile("junk/stuff.txt", "new\n");
  // Even though important.txt matches an include rule, the fact that it
  // is inside an excluded directory takes precedence.
  test.getMount().addFile("junk/important.txt", "new\n");

  auto result = test.diff();
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(std::make_pair("src/1.txt", ScmFileStatus::ADDED)));

  result = test.diff(true);
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("src/1.txt", ScmFileStatus::ADDED),
          std::make_pair("1.txt", ScmFileStatus::IGNORED),
          std::make_pair("ignore.txt", ScmFileStatus::IGNORED),
          std::make_pair("junk/stuff.txt", ScmFileStatus::IGNORED),
          std::make_pair("junk/important.txt", ScmFileStatus::IGNORED),
          std::make_pair("src/foo/ignore.txt", ScmFileStatus::IGNORED),
          std::make_pair(
              "src/foo/abc/xyz/ignore.txt", ScmFileStatus::IGNORED)));
}

// Test with a file that matches a .gitignore pattern but also is already in the
// Tree (so we should report the modification)
TEST(DiffTest, ignoredFileInMountAndInTree) {
  DiffTest test({
      {".gitignore", "/1.txt\nignore.txt\njunk/\n!important.txt\nxyz\n"},
      {"a/b.txt", "test\n"},
      {"src/x.txt", "test\n"},
      {"src/y.txt", "test\n"},
      {"src/z.txt", "test\n"},
      {"src/foo/abc/xyz/ignore.txt", "test\n"},
      {"src/foo/bar.txt", "test\n"},
  });

  // Add some untracked files, some of which match the ignore patterns
  test.getMount().addFile("1.txt", "new\n");
  test.getMount().addFile("ignore.txt", "new\n");
  test.getMount().addFile("src/1.txt", "new\n");
  test.getMount().addFile("src/foo/ignore.txt", "new\n");
  test.getMount().mkdir("junk");
  test.getMount().addFile("junk/stuff.txt", "new\n");

  // overwrite a file that already exists and matches the ignore pattern
  test.getMount().overwriteFile("src/foo/abc/xyz/ignore.txt", "modified\n");

  // Even though important.txt matches an include rule, the fact that it
  // is inside an excluded directory takes precedence.
  test.getMount().addFile("junk/important.txt", "new\n");

  auto result = test.diff();
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("src/1.txt", ScmFileStatus::ADDED),
          std::make_pair(
              "src/foo/abc/xyz/ignore.txt", ScmFileStatus::MODIFIED)));

  result = test.diff(true);
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("src/1.txt", ScmFileStatus::ADDED),
          std::make_pair("src/foo/abc/xyz/ignore.txt", ScmFileStatus::MODIFIED),
          std::make_pair("1.txt", ScmFileStatus::IGNORED),
          std::make_pair("ignore.txt", ScmFileStatus::IGNORED),
          std::make_pair("junk/stuff.txt", ScmFileStatus::IGNORED),
          std::make_pair("junk/important.txt", ScmFileStatus::IGNORED),
          std::make_pair("src/foo/ignore.txt", ScmFileStatus::IGNORED)));
}

// Test with a file that matches a .gitignore pattern but also is already in the
// Tree but removed from mount (so we should report the file removal)
TEST(DiffTest, ignoredFileNotInMountButInTree) {
  DiffTest test({
      {".gitignore", "/1.txt\nignore.txt\njunk/\n!important.txt\nxyz\n"},
      {"a/b.txt", "test\n"},
      {"src/x.txt", "test\n"},
      {"src/y.txt", "test\n"},
      {"src/z.txt", "test\n"},
      {"src/foo/abc/xyz/ignore.txt", "test\n"},
      {"src/foo/bar.txt", "test\n"},
  });

  // Add some untracked files, some of which match the ignore patterns
  test.getMount().addFile("1.txt", "new\n");
  test.getMount().addFile("ignore.txt", "new\n");
  test.getMount().addFile("src/1.txt", "new\n");
  test.getMount().addFile("src/foo/ignore.txt", "new\n");
  test.getMount().mkdir("junk");
  test.getMount().addFile("junk/stuff.txt", "new\n");

  // remove a file that already exists and matches the ignore pattern
  test.getMount().deleteFile("src/foo/abc/xyz/ignore.txt");

  // Even though important.txt matches an include rule, the fact that it
  // is inside an excluded directory takes precedence.
  test.getMount().addFile("junk/important.txt", "new\n");

  auto result = test.diff();
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("src/1.txt", ScmFileStatus::ADDED),
          std::make_pair(
              "src/foo/abc/xyz/ignore.txt", ScmFileStatus::REMOVED)));

  result = test.diff(true);
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("src/1.txt", ScmFileStatus::ADDED),
          std::make_pair("src/foo/abc/xyz/ignore.txt", ScmFileStatus::REMOVED),
          std::make_pair("1.txt", ScmFileStatus::IGNORED),
          std::make_pair("ignore.txt", ScmFileStatus::IGNORED),
          std::make_pair("junk/stuff.txt", ScmFileStatus::IGNORED),
          std::make_pair("junk/important.txt", ScmFileStatus::IGNORED),
          std::make_pair("src/foo/ignore.txt", ScmFileStatus::IGNORED)));
}

// Test with a .gitignore file in the top-level directory
// and the presence of none, either, or both of system level
// and user specific ignore files
TEST(DiffTest, ignoreSystemLevelAndUser) {
  DiffTest test({
      {".gitignore", "/1.txt\nignore.txt\njunk/\n!important.txt\n"},
      {"a/b.txt", "test\n"},
      {"src/x.txt", "test\n"},
      {"src/y.txt", "test\n"},
      {"src/z.txt", "test\n"},
      {"src/foo/bar.txt", "test\n"},
  });

  // Add some untracked files, matching either global or user patterns
  test.getMount().addFile("skip_global.txt", "new\n");
  test.getMount().addFile("skip_user.txt", "new\n");

  auto result =
      test.diff(true /* listIgnored */, "skip_global.txt\n", "skip_user.txt\n");
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("skip_global.txt", ScmFileStatus::IGNORED),
          std::make_pair("skip_user.txt", ScmFileStatus::IGNORED)));

  result = test.diff(true /* listIgnored */, "", "skip_user.txt\n");
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("skip_global.txt", ScmFileStatus::ADDED),
          std::make_pair("skip_user.txt", ScmFileStatus::IGNORED)));

  result = test.diff(true /* listIgnored */, "skip_global.txt\n", "");
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("skip_global.txt", ScmFileStatus::IGNORED),
          std::make_pair("skip_user.txt", ScmFileStatus::ADDED)));

  result = test.diff(true /* listIgnored */, "", "");
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("skip_global.txt", ScmFileStatus::ADDED),
          std::make_pair("skip_user.txt", ScmFileStatus::ADDED)));
}

#ifndef _WIN32
// Test gitignore file which is a symlink. Symlinked gitignore are ignored.
TEST(DiffTest, ignoreSymlink) {
  DiffTest test({
      {"actual", "/1.txt\nignore.txt\njunk/\n!important.txt\n"},
      {"a/b.txt", "test\n"},
      {"src/x.txt", "test\n"},
      {"src/y.txt", "test\n"},
      {"src/z.txt", "test\n"},
      {"src/foo/bar.txt", "test\n"},
  });
  test.getMount().addFile("1.txt", "new\n");
  test.getMount().addFile("ignore.txt", "new\n");

  test.getMount().addSymlink(".gitignore", "a/second");
  test.getMount().addSymlink("a/second", "../actual");
  test.getMount().addSymlink("a/.gitignore", ".gitignore");
  test.getMount().mkdir("b");
  test.getMount().addSymlink("b/.gitignore", "../b");
  test.getMount().addSymlink("src/.gitignore", "broken/link/to/nowhere");

  auto result = test.diff();
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair(".gitignore", ScmFileStatus::ADDED),
          std::make_pair("1.txt", ScmFileStatus::ADDED),
          std::make_pair("a/.gitignore", ScmFileStatus::ADDED),
          std::make_pair("a/second", ScmFileStatus::ADDED),
          std::make_pair("b/.gitignore", ScmFileStatus::ADDED),
          std::make_pair("ignore.txt", ScmFileStatus::ADDED),
          std::make_pair("src/.gitignore", ScmFileStatus::ADDED)));

  result = test.diff(true);
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair(".gitignore", ScmFileStatus::ADDED),
          std::make_pair("a/.gitignore", ScmFileStatus::ADDED),
          std::make_pair("a/second", ScmFileStatus::ADDED),
          std::make_pair("b/.gitignore", ScmFileStatus::ADDED),
          std::make_pair("src/.gitignore", ScmFileStatus::ADDED),
          std::make_pair("1.txt", ScmFileStatus::ADDED),
          std::make_pair("ignore.txt", ScmFileStatus::ADDED)));
}
#endif // !_WIN32

// Test with a .gitignore file in the top-level directory
TEST(DiffTest, ignoreInSubdirectories) {
  DiffTest test({
      {".gitignore", "**/foo/bar.txt\n"},
      {"foo/.gitignore", "stuff\ntest\nwhatever\n"},
      {"foo/foo/.gitignore", "!/bar.txt\ntest\n"},
      {"abc/def/.gitignore", "*.log\n"},
      {"abc/def/other.txt", "test\n"},
      {"a/.gitignore", "b/c/d.txt\n"},
      {"a/b/c/x.txt", "test\n"},
      {"b/c/x.txt", "test\n"},
  });

  // Add some untracked files, some of which match the ignore patterns
  test.getMount().addFile("foo/bar.txt", "new\n");
  test.getMount().addFile("foo/foo/bar.txt", "new\n");
  test.getMount().mkdir("foo/test");
  test.getMount().addFile("foo/test/1.txt", "new\n");
  test.getMount().addFile("foo/test/2.txt", "new\n");
  test.getMount().mkdir("foo/test/3");
  test.getMount().addFile("foo/test/3/4.txt", "new\n");
  test.getMount().addFile("foo/foo/test", "new\n");
  test.getMount().addFile("test", "test\n");
  test.getMount().addFile("abc/def/test", "test\n");
  test.getMount().addFile("abc/def/test.log", "test\n");
  test.getMount().addFile("abc/def/another.log", "test\n");
  test.getMount().addFile("abc/test.log", "test\n");
  test.getMount().mkdir("abc/foo");
  test.getMount().addFile("abc/foo/bar.txt", "test\n");
  test.getMount().mkdir("other");
  test.getMount().addFile("other/bar.txt", "test\n");
  test.getMount().addFile("a/b/c/d.txt", "test\n");
  test.getMount().addFile("b/c/d.txt", "test\n");

  auto result = test.diff();
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("abc/test.log", ScmFileStatus::ADDED),
          std::make_pair("abc/def/test", ScmFileStatus::ADDED),
          std::make_pair("b/c/d.txt", ScmFileStatus::ADDED),
          // Matches exclude rule in top-level .gitignore, but explicitly
          // included by "!bar.txt" rule in foo/foo/.gitignore
          std::make_pair("foo/foo/bar.txt", ScmFileStatus::ADDED),
          std::make_pair("other/bar.txt", ScmFileStatus::ADDED),
          std::make_pair("test", ScmFileStatus::ADDED)));

  result = test.diff(true);
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAreArray(
          {std::make_pair("abc/test.log", ScmFileStatus::ADDED),
           std::make_pair("abc/def/test", ScmFileStatus::ADDED),
           std::make_pair("b/c/d.txt", ScmFileStatus::ADDED),
           std::make_pair("foo/foo/bar.txt", ScmFileStatus::ADDED),
           std::make_pair("other/bar.txt", ScmFileStatus::ADDED),
           std::make_pair("test", ScmFileStatus::ADDED),
           std::make_pair("a/b/c/d.txt", ScmFileStatus::IGNORED),
           // Ignored by "*.log" rule in abc/def/.gitignore
           std::make_pair("abc/def/test.log", ScmFileStatus::IGNORED),
           std::make_pair("abc/def/another.log", ScmFileStatus::IGNORED),
           // Ignored by "**/foo/bar.txt" rule in top-level .gitignore file
           std::make_pair("abc/foo/bar.txt", ScmFileStatus::IGNORED),
           // Ignored by "**/foo/bar.txt" rule in top-level .gitignore file
           std::make_pair("foo/bar.txt", ScmFileStatus::IGNORED),
           // Ignored by "test" rule in foo/.gitignore
           std::make_pair("foo/test/1.txt", ScmFileStatus::IGNORED),
           std::make_pair("foo/test/2.txt", ScmFileStatus::IGNORED),
           std::make_pair("foo/test/3/4.txt", ScmFileStatus::IGNORED),
           // Also ignored by "test" rule in foo/.gitignore
           std::make_pair("foo/foo/test", ScmFileStatus::IGNORED)}));
}

// Test with a .gitignore in subdirectories and file exists both in mount and in
// the Tree (so we should report the file modification)
TEST(DiffTest, ignoreInSubdirectoriesInMountAndInTree) {
  DiffTest test(
      {{".gitignore", "**/foo/bar.txt\n"},
       {"foo/.gitignore", "stuff\ntest\nwhatever\n"},
       {"foo/foo/.gitignore", "!/bar.txt\ntest\n"},
       {"abc/def/.gitignore", "*.log\n"},
       {"abc/def/other.txt", "test\n"},
       {"a/.gitignore", "b/c/d.txt\n"},
       {"a/b/c/x.txt", "test\n"},
       {"b/c/x.txt", "test\n"},
       {"abc/def/test.log", "test\n"}});

  // Add some untracked files, some of which match the ignore patterns
  test.getMount().addFile("foo/bar.txt", "new\n");
  test.getMount().addFile("foo/foo/bar.txt", "new\n");
  test.getMount().mkdir("foo/test");
  test.getMount().addFile("foo/test/1.txt", "new\n");
  test.getMount().addFile("foo/test/2.txt", "new\n");
  test.getMount().mkdir("foo/test/3");
  test.getMount().addFile("foo/test/3/4.txt", "new\n");
  test.getMount().addFile("foo/foo/test", "new\n");
  test.getMount().addFile("test", "test\n");
  test.getMount().addFile("abc/def/test", "test\n");
  test.getMount().addFile("abc/def/another.log", "test\n");
  test.getMount().addFile("abc/test.log", "test\n");
  test.getMount().mkdir("abc/foo");
  test.getMount().addFile("abc/foo/bar.txt", "test\n");
  test.getMount().mkdir("other");
  test.getMount().addFile("other/bar.txt", "test\n");
  test.getMount().addFile("a/b/c/d.txt", "test\n");
  test.getMount().addFile("b/c/d.txt", "test\n");

  // Overwrite a file that matches a .gitignore rule
  // Ignored by "*.log" rule in abc/def/.gitignore
  test.getMount().overwriteFile("abc/def/test.log", "changed\n");

  auto result = test.diff();
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("abc/test.log", ScmFileStatus::ADDED),
          std::make_pair("abc/def/test", ScmFileStatus::ADDED),
          std::make_pair("b/c/d.txt", ScmFileStatus::ADDED),
          // Matches exclude rule in top-level .gitignore, but explicitly
          // included by "!bar.txt" rule in foo/foo/.gitignore
          std::make_pair("foo/foo/bar.txt", ScmFileStatus::ADDED),
          std::make_pair("other/bar.txt", ScmFileStatus::ADDED),
          std::make_pair("test", ScmFileStatus::ADDED),
          std::make_pair("abc/def/test.log", ScmFileStatus::MODIFIED)));

  result = test.diff(true);
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAreArray(
          {std::make_pair("abc/test.log", ScmFileStatus::ADDED),
           std::make_pair("abc/def/test", ScmFileStatus::ADDED),
           std::make_pair("b/c/d.txt", ScmFileStatus::ADDED),
           std::make_pair("foo/foo/bar.txt", ScmFileStatus::ADDED),
           std::make_pair("other/bar.txt", ScmFileStatus::ADDED),
           std::make_pair("test", ScmFileStatus::ADDED),
           std::make_pair("abc/def/test.log", ScmFileStatus::MODIFIED),
           std::make_pair("a/b/c/d.txt", ScmFileStatus::IGNORED),
           // Ignored by "*.log" rule in abc/def/.gitignore
           std::make_pair("abc/def/another.log", ScmFileStatus::IGNORED),
           // Ignored by "**/foo/bar.txt" rule in top-level .gitignore file
           std::make_pair("abc/foo/bar.txt", ScmFileStatus::IGNORED),
           // Ignored by "**/foo/bar.txt" rule in top-level .gitignore file
           std::make_pair("foo/bar.txt", ScmFileStatus::IGNORED),
           // Ignored by "test" rule in foo/.gitignore
           std::make_pair("foo/test/1.txt", ScmFileStatus::IGNORED),
           std::make_pair("foo/test/2.txt", ScmFileStatus::IGNORED),
           std::make_pair("foo/test/3/4.txt", ScmFileStatus::IGNORED),
           // Also ignored by "test" rule in foo/.gitignore
           std::make_pair("foo/foo/test", ScmFileStatus::IGNORED)}));
}

// Test with a .gitignore in subdirectories and file exists not in mount but
// exists in the Tree (so we should report the file deletion)
TEST(DiffTest, ignoreInSubdirectoriesNotInMountButInTree) {
  DiffTest test(
      {{".gitignore", "**/foo/bar.txt\n"},
       {"foo/.gitignore", "stuff\ntest\nwhatever\n"},
       {"foo/foo/.gitignore", "!/bar.txt\ntest\n"},
       {"abc/def/.gitignore", "*.log\n"},
       {"abc/def/other.txt", "test\n"},
       {"a/.gitignore", "b/c/d.txt\n"},
       {"a/b/c/x.txt", "test\n"},
       {"b/c/x.txt", "test\n"},
       {"abc/def/test.log", "test\n"}});

  // Add some untracked files, some of which match the ignore patterns
  test.getMount().addFile("foo/bar.txt", "new\n");
  test.getMount().addFile("foo/foo/bar.txt", "new\n");
  test.getMount().mkdir("foo/test");
  test.getMount().addFile("foo/test/1.txt", "new\n");
  test.getMount().addFile("foo/test/2.txt", "new\n");
  test.getMount().mkdir("foo/test/3");
  test.getMount().addFile("foo/test/3/4.txt", "new\n");
  test.getMount().addFile("foo/foo/test", "new\n");
  test.getMount().addFile("test", "test\n");
  test.getMount().addFile("abc/def/test", "test\n");
  test.getMount().addFile("abc/def/another.log", "test\n");
  test.getMount().addFile("abc/test.log", "test\n");
  test.getMount().mkdir("abc/foo");
  test.getMount().addFile("abc/foo/bar.txt", "test\n");
  test.getMount().mkdir("other");
  test.getMount().addFile("other/bar.txt", "test\n");
  test.getMount().addFile("a/b/c/d.txt", "test\n");
  test.getMount().addFile("b/c/d.txt", "test\n");

  // Remove a file that matches a .gitignore rule
  // Ignored by "*.log" rule in abc/def/.gitignore
  test.getMount().deleteFile("abc/def/test.log");

  auto result = test.diff();
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("abc/test.log", ScmFileStatus::ADDED),
          std::make_pair("abc/def/test", ScmFileStatus::ADDED),
          std::make_pair("b/c/d.txt", ScmFileStatus::ADDED),
          // Matches exclude rule in top-level .gitignore, but explicitly
          // included by "!bar.txt" rule in foo/foo/.gitignore
          std::make_pair("foo/foo/bar.txt", ScmFileStatus::ADDED),
          std::make_pair("other/bar.txt", ScmFileStatus::ADDED),
          std::make_pair("test", ScmFileStatus::ADDED),
          std::make_pair("abc/def/test.log", ScmFileStatus::REMOVED)));

  result = test.diff(true);
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAreArray(
          {std::make_pair("abc/test.log", ScmFileStatus::ADDED),
           std::make_pair("abc/def/test", ScmFileStatus::ADDED),
           std::make_pair("b/c/d.txt", ScmFileStatus::ADDED),
           std::make_pair("foo/foo/bar.txt", ScmFileStatus::ADDED),
           std::make_pair("other/bar.txt", ScmFileStatus::ADDED),
           std::make_pair("test", ScmFileStatus::ADDED),
           std::make_pair("abc/def/test.log", ScmFileStatus::REMOVED),
           std::make_pair("a/b/c/d.txt", ScmFileStatus::IGNORED),
           // Ignored by "*.log" rule in abc/def/.gitignore
           std::make_pair("abc/def/another.log", ScmFileStatus::IGNORED),
           // Ignored by "**/foo/bar.txt" rule in top-level .gitignore file
           std::make_pair("abc/foo/bar.txt", ScmFileStatus::IGNORED),
           // Ignored by "**/foo/bar.txt" rule in top-level .gitignore file
           std::make_pair("foo/bar.txt", ScmFileStatus::IGNORED),
           // Ignored by "test" rule in foo/.gitignore
           std::make_pair("foo/test/1.txt", ScmFileStatus::IGNORED),
           std::make_pair("foo/test/2.txt", ScmFileStatus::IGNORED),
           std::make_pair("foo/test/3/4.txt", ScmFileStatus::IGNORED),
           // Also ignored by "test" rule in foo/.gitignore
           std::make_pair("foo/foo/test", ScmFileStatus::IGNORED)}));
}

// Test when files already tracked in source control match ignore patterns
TEST(DiffTest, explicitlyTracked) {
  DiffTest test({
      {".gitignore", "1.txt\njunk\n"},
      {"junk/a/b/c.txt", "test\n"},
      {"junk/a/b/d.txt", "test\n"},
      {"junk/x/foo.txt", "test\n"},
      {"src/1.txt", "test\n"},
      {"docs/test.txt", "test\n"},
  });

  test.getMount().addFile("docs/1.txt", "new\n");
  test.getMount().addFile("junk/foo.txt", "new\n");
  test.getMount().addFile("junk/test.txt", "new\n");
  test.getMount().addFile("junk/a/b/xyz.txt", "new\n");
  test.getMount().addFile("other.txt", "new\n");
  test.getMount().overwriteFile("junk/a/b/c.txt", "new\n");
  test.getMount().deleteFile("junk/x/foo.txt");

  auto result = test.diff();
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("other.txt", ScmFileStatus::ADDED),
          std::make_pair("junk/x/foo.txt", ScmFileStatus::REMOVED),
          std::make_pair("junk/a/b/c.txt", ScmFileStatus::MODIFIED)));

  result = test.diff(true);
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("other.txt", ScmFileStatus::ADDED),
          std::make_pair("docs/1.txt", ScmFileStatus::IGNORED),
          std::make_pair("junk/foo.txt", ScmFileStatus::IGNORED),
          std::make_pair("junk/test.txt", ScmFileStatus::IGNORED),
          std::make_pair("junk/a/b/xyz.txt", ScmFileStatus::IGNORED),
          std::make_pair("junk/x/foo.txt", ScmFileStatus::REMOVED),
          std::make_pair("junk/a/b/c.txt", ScmFileStatus::MODIFIED)));
}

// Test making modifications to the .gitignore file
TEST(DiffTest, ignoreFileModified) {
  DiffTest test({
      {"a/.gitignore", "foo.txt\n"},
  });

  test.getMount().addFile("a/foo.txt", "test\n");
  test.getMount().addFile("a/bar.txt", "test\n");
  test.getMount().addFile("a/test.txt", "test\n");

  auto result = test.diff(true);
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("a/bar.txt", ScmFileStatus::ADDED),
          std::make_pair("a/test.txt", ScmFileStatus::ADDED),
          std::make_pair("a/foo.txt", ScmFileStatus::IGNORED)));

  // Changes to the gitignore file should take effect immediately
  test.getMount().overwriteFile("a/.gitignore", "bar.txt\n");

  result = test.diff(true);
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("a/test.txt", ScmFileStatus::ADDED),
          std::make_pair("a/foo.txt", ScmFileStatus::ADDED),
          std::make_pair("a/bar.txt", ScmFileStatus::IGNORED),
          std::make_pair("a/.gitignore", ScmFileStatus::MODIFIED)));

  // Newly added gitignore files should also take effect immediately
  test.getMount().addFile(".gitignore", "test.txt\n");

  result = test.diff(true);
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair(".gitignore", ScmFileStatus::ADDED),
          std::make_pair("a/foo.txt", ScmFileStatus::ADDED),
          std::make_pair("a/bar.txt", ScmFileStatus::IGNORED),
          std::make_pair("a/test.txt", ScmFileStatus::IGNORED),
          std::make_pair("a/.gitignore", ScmFileStatus::MODIFIED)));
}

// Make sure the code ignores .gitignore directories
TEST(DiffTest, ignoreFileIsDirectory) {
  DiffTest test({
      {".gitignore", "1.txt\nignore.txt\n"},
      {"a/b.txt", "test\n"},
      {"a/.gitignore/b.txt", "test\n"},
      {"a/b/c.txt", "test\n"},
  });

  test.getMount().addFile("a/b/1.txt", "new\n");

  auto result = test.diff();
  EXPECT_THAT(*result.entries(), UnorderedElementsAre());

  result = test.diff(true);
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("a/b/1.txt", ScmFileStatus::IGNORED)));
}

TEST(DiffTest, emptyIgnoreFile) {
  DiffTest test({
      {"src/foo.txt", "test\n"},
      {"src/subdir/bar.txt", "test\n"},
      {"src/.gitignore", ""},
  });

  test.getMount().addFile("src/subdir/new.txt", "new\n");

  auto result = test.diff();
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("src/subdir/new.txt", ScmFileStatus::ADDED)));
}

#ifndef _WIN32
// Disabling on Windows as writing files with \r is too painful:
// https://stackoverflow.com/questions/21571999/handling-files-with-carriage-return-in-filename-on-windows
TEST(DiffTest, ignoredFilePatternCarriageReturn) {
  DiffTest test({
      {"src/foo.txt", "test\n"},
      {"src/.gitignore", "Icon[\r]"},
  });

  test.getMount().addFile("src/Icon\r", "");

  auto result = test.diff();
  EXPECT_THAT(*result.entries(), UnorderedElementsAre());

  result = test.diff(true);
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("src/Icon\r", ScmFileStatus::IGNORED)));
}

TEST(DiffTest, ignoredFileDoubleCarriageReturn) {
  DiffTest test({
      {"src/foo.txt", "test\n"},
      {"src/.gitignore", "Icon\r\r"},
  });

  test.getMount().addFile("src/Icon\r", "");

  auto result = test.diff();
  EXPECT_THAT(*result.entries(), UnorderedElementsAre());

  result = test.diff(true);
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("src/Icon\r", ScmFileStatus::IGNORED)));
}

TEST(DiffTest, ignoredFileSingleCarriageReturn) {
  DiffTest test({
      {"src/foo.txt", "test\n"},
      {"src/.gitignore", "Icon\r"},
  });

  test.getMount().addFile("src/Icon\r", "");

  auto result = test.diff();
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(std::make_pair("src/Icon\r", ScmFileStatus::ADDED)));
}
#endif

// Files under the .hg directory should never be reported in diff results
TEST(DiffTest, ignoreHidden) {
  DiffTest test({
      {"a/b.txt", "test\n"},
      {"a/c/d.txt", "test\n"},
      {"a/c/1.txt", "test\n"},
      {"a/c/2.txt", "test\n"},
  });

  test.getMount().mkdir(".hg");
  test.getMount().addFile(".hg/hgrc", "# hgrc contents would go here\n");
  test.getMount().addFile(".hg/bookmarks", "123456789 foobar\n");
  test.getMount().mkdir(".hg/store");
  test.getMount().mkdir(".hg/store/data");
  test.getMount().addFile(".hg/store/data/00changelog.d", "stuff\n");
  test.getMount().addFile(".hg/store/data/00changelog.i", "stuff\n");

  test.getMount().overwriteFile("a/c/1.txt", "updated contents\n");

  auto result = test.diff(true);
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("a/c/1.txt", ScmFileStatus::MODIFIED)));
}

// Tests the case in which a tracked directory in source control is replaced by
// a file locally, and the directory matches an ignore rule. In this case,
// the file should be recorded as ADDED, since the ignore rule is specifically
// for directories
TEST(DiffTest, directoryToFileWithGitIgnore) {
  DiffTest test({
      {"a/b.txt", "test\n"},
      {"a/b/c.txt", "test\n"},
      {"a/b/d.txt", "test\n"},
  });

  test.getMount().deleteFile("a/b/c.txt");
  test.getMount().deleteFile("a/b/d.txt");
  test.getMount().rmdir("a/b");
  test.getMount().addFile("a/b", "regular file");
  test.getMount().addFile(".gitignore", "a/b/");

  auto result = test.diff();
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("a/b/c.txt", ScmFileStatus::REMOVED),
          std::make_pair("a/b/d.txt", ScmFileStatus::REMOVED),
          std::make_pair("a/b", ScmFileStatus::ADDED),
          std::make_pair(".gitignore", ScmFileStatus::ADDED)));

  result = test.diff(true);
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("a/b/c.txt", ScmFileStatus::REMOVED),
          std::make_pair("a/b/d.txt", ScmFileStatus::REMOVED),
          std::make_pair("a/b", ScmFileStatus::ADDED),
          std::make_pair(".gitignore", ScmFileStatus::ADDED)));
}

// Tests the case in which a file becomes a directory and the directory is
// ignored but the parent directory is not ignored.
TEST(DiffTest, addIgnoredDirectory) {
  DiffTest test({
      {"a/b.txt", "test\n"},
      {"a/b/c.txt", "test\n"},
      {"a/b/r", "test\n"},
  });

  // The following won't be tracked
  test.getMount().deleteFile("a/b/r");
  test.getMount().mkdir("a/b/r");
  test.getMount().addFile("a/b/r/e.txt", "ignored");
  test.getMount().mkdir("a/b/r/d");
  // It is not possible to re-include a file if a parent directory of that file
  // is excluded.
  test.getMount().addFile("a/b/r/d/g.txt", "ignored too");

  // The following should be tracked
  test.getMount().mkdir("a/b/g");
  test.getMount().addFile("a/b/g/e.txt", "added");

  auto systemIgnore = "a/b/r/\n!a/b/r/d/g.txt\n";
  auto result = test.diff(true, systemIgnore);

  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("a/b/r", ScmFileStatus::REMOVED),
          std::make_pair("a/b/r/e.txt", ScmFileStatus::IGNORED),
          std::make_pair("a/b/r/d/g.txt", ScmFileStatus::IGNORED),
          std::make_pair("a/b/g/e.txt", ScmFileStatus::ADDED)));
}

// Tests the case in which a directory is ignored but later down a file is
// unignored, makes sure that the file is correctly marked as added.
TEST(DiffTest, nestedGitIgnoreFiles) {
  DiffTest test({
      {"a/b.txt", "test\n"},
      {"a/b/c.txt", "test\n"},
      {"a/b/r", "file"},
  });

  auto gitIgnoreContents = "!e.txt\n";
  test.getMount().deleteFile("a/b/r");
  test.getMount().mkdir("a/b/r");
  test.getMount().addFile("a/b/r/.gitignore", gitIgnoreContents);
  test.getMount().addFile("a/b/r/e.txt", "shouldn't be ignored");
  test.getMount().addFile("a/b/r/f.txt", "should be ignored");

  auto systemIgnore = "a/b/r/*\n!a/b/r/.gitignore\n";
  auto result = test.diff(true, systemIgnore);
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("a/b/r", ScmFileStatus::REMOVED),
          std::make_pair("a/b/r/.gitignore", ScmFileStatus::ADDED),
          std::make_pair("a/b/r/e.txt", ScmFileStatus::ADDED),
          std::make_pair("a/b/r/f.txt", ScmFileStatus::IGNORED)));
}

// Tests the case in which a tracked file in source control is modified locally.
// In this case, the file should be recorded as MODIFIED, since it matches
// an ignore rule but was already tracked
TEST(DiffTest, diff_trees_with_tracked_ignored_file_modified) {
  DiffTest test({
      {"src/foo/a.txt", "a"},
      {"src/foo/b.txt", "b"},
      {"src/foo/a", "regular file"},
      {"src/bar/c.txt", "c"},
      {"src/bar/d.txt", "d"},
      {"src/bar/c", "regular file"},
      {"src/foo/.gitignore", "a.txt\n"},

  });

  test.getMount().addFile("src/bar/e.txt", "e");
  test.getMount().deleteFile("src/bar/d.txt");

  // Even though this is modified, it will be ignored because it matches an
  // ignore rule.
  test.getMount().overwriteFile("src/foo/a.txt", "aa");

  auto result = test.diff();
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("src/bar/e.txt", ScmFileStatus::ADDED),
          std::make_pair("src/bar/d.txt", ScmFileStatus::REMOVED),
          std::make_pair("src/foo/a.txt", ScmFileStatus::MODIFIED)));

  result = test.diff(true);
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("src/bar/e.txt", ScmFileStatus::ADDED),
          std::make_pair("src/bar/d.txt", ScmFileStatus::REMOVED),
          std::make_pair("src/foo/a.txt", ScmFileStatus::MODIFIED)));
}

// Tests the case in which a tracked file in source control is modified locally.
// In this case, the file should be recorded as MODIFIED, since it matches
// an ignore rule but was already tracked
TEST(DiffTest, tree_file_matches_new_ignore_rule_modified_locally) {
  DiffTest test({
      {"src/foo/a.txt", "a"},
      {"src/foo/b.txt", "b"},
      {"src/foo/a", "regular file"},
      {"src/bar/c.txt", "c"},
      {"src/bar/d.txt", "d"},
      {"src/bar/c", "regular file"},

  });

  test.getMount().addFile("src/foo/.gitignore", "a.txt\n");
  test.getMount().addFile("src/bar/e.txt", "e");
  test.getMount().deleteFile("src/bar/d.txt");

  // Even though this is modified, it will be ignored because it matches an
  // ignore rule.
  test.getMount().overwriteFile("src/foo/a.txt", "aa");

  auto result = test.diff();
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("src/foo/.gitignore", ScmFileStatus::ADDED),
          std::make_pair("src/bar/e.txt", ScmFileStatus::ADDED),
          std::make_pair("src/bar/d.txt", ScmFileStatus::REMOVED),
          std::make_pair("src/foo/a.txt", ScmFileStatus::MODIFIED)));

  result = test.diff(true);
  EXPECT_THAT(
      *result.entries(),
      UnorderedElementsAre(
          std::make_pair("src/foo/.gitignore", ScmFileStatus::ADDED),
          std::make_pair("src/bar/e.txt", ScmFileStatus::ADDED),
          std::make_pair("src/bar/d.txt", ScmFileStatus::REMOVED),
          std::make_pair("src/foo/a.txt", ScmFileStatus::MODIFIED)));
}

TEST(DiffTest, fileNotReady) {
  TestMount mount;
  auto backingStore = mount.getBackingStore();

  // Create two trees to diff
  FakeTreeBuilder builder1;
  builder1.setFiles({
      // In src/ we will have some non-materialized files that are modified
      // in builder2's tree.
      {"src/r.txt", "This is src/r.txt.\n"},
      {"src/s.txt", "This is src/s.txt.\n"},
      {"src/t.txt", "This is src/t.txt.\n"},
      {"src/u.txt", "This is src/u.txt.\n"},
      // In doc/ we will have some materialized files that are modified.
      {"doc/a.txt", "This is doc/a.txt.\n"},
      {"doc/b.txt", "This is doc/b.txt.\n"},
      {"doc/c.txt", "This is doc/c.txt.\n"},
      {"doc/d.txt", "This is doc/d.txt.\n"},
      {"other/x/y/z.txt", "other\n"},
      {"toplevel.txt", "toplevel\n"},
  });
  auto builder2 = builder1.clone();
  builder2.replaceFile("src/r.txt", "src/r.txt has been updated.\n");
  builder2.replaceFile("src/s.txt", "src/s.txt has also been updated.\n");
  builder2.replaceFile("src/t.txt", "src/t.txt updated.\n");
  builder2.replaceFile("src/u.txt", "src/u.txt updated.\n");
  builder2.replaceFile("doc/a.txt", "a.txt modified in builder2.\n");
  builder2.replaceFile("doc/b.txt", "b.txt modified in builder2.\n");

  // Set the mount pointing to the first tree
  mount.initialize(builder1, /*startReady=*/false);

  // Locally modify some of the files under doc/
  // We need to make the blobs ready in order to modify the inodes,
  // but mark them not ready again afterwards.
  builder1.setReady("doc");
  auto a1 = builder1.getStoredBlob("doc/a.txt"_relpath);
  auto b1 = builder1.getStoredBlob("doc/b.txt"_relpath);
  auto c1 = builder1.getStoredBlob("doc/c.txt"_relpath);
  auto d1 = builder1.getStoredBlob("doc/d.txt"_relpath);
  a1->setReady();
  b1->setReady();
  c1->setReady();
  d1->setReady();
  mount.overwriteFile("doc/a.txt", "updated a.txt\n");
  mount.overwriteFile("doc/b.txt", "updated b.txt\n");
  mount.overwriteFile("doc/c.txt", "updated c.txt\n");
  mount.overwriteFile("doc/d.txt", "updated d.txt\n");
  a1->notReady();
  b1->notReady();
  c1->notReady();
  d1->notReady();

  // Load r.txt and s.txt
  builder1.setReady("src");
  auto r1 = builder1.getStoredBlob("src/r.txt"_relpath);
  auto s1 = builder1.getStoredBlob("src/s.txt"_relpath);
  r1->setReady();
  s1->setReady();
  auto r1inode = mount.getInode("src/r.txt"_relpath);
  auto s1inode = mount.getInode("src/s.txt"_relpath);
  r1->notReady();
  s1->notReady();

  // Add tree2 to the backing store and create a commit pointing to it.
  auto rootTree2 = builder2.finalize(backingStore, /*setReady=*/false);
  auto commitId2 = mount.nextCommitId();
  auto* commit2 =
      backingStore->putCommit(commitId2, rootTree2->get().getObjectId());
  commit2->setReady();
  builder2.getRoot()->setReady();

  // Run the diff
  auto diffFuture = mount.getEdenMount()
                        ->diff(
                            mount.getRootInode(),
                            commitId2,
                            folly::CancellationToken{},
                            ObjectFetchContext::getNullContext(),
                            /*listIgnored=*/false,
                            /*enforceCurrentParent=*/false)
                        .semi()
                        .via(mount.getServerExecutor().get());
  mount.drainServerExecutor();

  // The diff should not be ready yet
  EXPECT_FALSE(diffFuture.isReady());

  // other/ and toplevel.txt are not modified, so they share the same objects in
  // builder1 and builder2.  We only need to mark them ready via one of the two
  // builders.
  builder1.setReady("other");
  builder1.setReady("toplevel.txt");

  // The src/ and doc/ directories are different between the two builders.
  // Mark them ready in each builder.
  builder1.setReady("src");
  builder2.setReady("src");
  builder1.setReady("doc");
  builder2.setReady("doc");

  mount.drainServerExecutor();

  EXPECT_FALSE(diffFuture.isReady());

  // Process the modified files in src/
  // These inodes are not materialized.  r.txt and s.txt have been loaded.
  auto r2 = builder2.getStoredBlob("src/r.txt"_relpath);
  auto s2 = builder2.getStoredBlob("src/s.txt"_relpath);
  auto t2 = builder2.getStoredBlob("src/t.txt"_relpath);
  auto u2 = builder2.getStoredBlob("src/u.txt"_relpath);
  auto t1 = builder1.getStoredBlob("src/t.txt"_relpath);
  auto u1 = builder1.getStoredBlob("src/u.txt"_relpath);

  // The diff process calls both getBlob() and getSha1(), which can end
  // up waiting on these objects to load multiple times.
  //
  // trigger these objects multiple times without marking them fully ready yet.
  // This causes the diff process to make forward progress while still resulting
  // in non-ready futures internally that must be waited for.
  const unsigned int numTriggers = 5;
  for (unsigned int n = 0; n < numTriggers; ++n) {
    r1->trigger();
    r2->trigger();

    s2->trigger();
    s1->trigger();

    t1->trigger();
    t2->trigger();

    u2->trigger();
    u1->trigger();
  }

  mount.drainServerExecutor();

  EXPECT_FALSE(diffFuture.isReady());

  // Process the modified files under doc/
  // The inodes for these files are materialized, which triggers a different
  // code path than for non-materialized files.
  auto a2 = builder2.getStoredBlob("doc/a.txt"_relpath);
  auto b2 = builder2.getStoredBlob("doc/b.txt"_relpath);
  auto c2 = builder2.getStoredBlob("doc/c.txt"_relpath);
  auto d2 = builder2.getStoredBlob("doc/d.txt"_relpath);
  for (unsigned int n = 0; n < numTriggers; ++n) {
    a2->trigger();
    b2->trigger();
    c2->trigger();
    d2->trigger();
  }

  // The diff should generally be ready at this point
  // However explicitly mark all objects as ready just in case.
  builder1.setAllReady();
  builder2.setAllReady();

  mount.drainServerExecutor();

  // The diff should be complete now.
  ASSERT_TRUE(diffFuture.isReady());
  auto result = std::move(diffFuture).get(10ms);

  EXPECT_THAT(
      *result->entries(),
      UnorderedElementsAre(
          std::make_pair("src/r.txt", ScmFileStatus::MODIFIED),
          std::make_pair("src/s.txt", ScmFileStatus::MODIFIED),
          std::make_pair("src/t.txt", ScmFileStatus::MODIFIED),
          std::make_pair("src/u.txt", ScmFileStatus::MODIFIED),
          std::make_pair("doc/a.txt", ScmFileStatus::MODIFIED),
          std::make_pair("doc/b.txt", ScmFileStatus::MODIFIED),
          std::make_pair("doc/c.txt", ScmFileStatus::MODIFIED),
          std::make_pair("doc/d.txt", ScmFileStatus::MODIFIED)));
}

TEST(DiffTest, cancelledDiff) {
  TestMount mount;
  auto backingStore = mount.getBackingStore();

  FakeTreeBuilder builder1;
  builder1.setFiles({
      {"a.txt", "a.txt from builder1\n"},
      {"src/b.txt", "src/b.txt from builder1\n"},
  });

  auto builder2 = builder1.clone();
  builder2.replaceFile("a.txt", "a.txt from builder2\n");
  builder2.replaceFile("src/b.txt", "src/b.txt from builder2\n");

  mount.initialize(builder1);

  auto rootTree2 = builder2.finalize(backingStore, /*setReady=*/false);
  auto commitId2 = mount.nextCommitId();
  auto* commit2 =
      backingStore->putCommit(commitId2, rootTree2->get().getObjectId());
  commit2->setReady();
  builder2.getRoot()->setReady();

  auto cancellationSource = folly::CancellationSource{};

  auto diffFuture = mount.getEdenMount()
                        ->diff(
                            mount.getRootInode(),
                            commitId2,
                            cancellationSource.getToken(),
                            ObjectFetchContext::getNullContext(),
                            /*listIgnored=*/false,
                            /*enforceCurrentParent=*/false)
                        .semi()
                        .via(mount.getServerExecutor().get());
  mount.drainServerExecutor();
  EXPECT_FALSE(diffFuture.isReady());

  cancellationSource.requestCancellation();
  mount.drainServerExecutor();
  EXPECT_FALSE(diffFuture.isReady());

  builder2.setReady("a.txt");
  mount.drainServerExecutor();
  EXPECT_FALSE(diffFuture.isReady());
  builder2.setReady("src");
  mount.drainServerExecutor();
  EXPECT_TRUE(diffFuture.isReady());

  auto result = std::move(diffFuture).get(10ms);

  EXPECT_THAT(
      *result->entries(),
      UnorderedElementsAre(std::make_pair("a.txt", ScmFileStatus::MODIFIED)));
}

class DiffTestNonMateralized : public ::testing::Test {
 protected:
  void SetUp() override {
    builder_.setFiles({
        {"root/src/r.txt", "This is src/r.txt.\n"},
        {"root/src/s.txt", "This is src/s.txt.\n"},
        {"root/src/t.txt", "This is src/t.txt.\n"},
        {"root/src/u.txt", "This is src/u.txt.\n"},
        {"root/doc/a.txt", "This is doc/a.txt.\n"},
        {"root/doc/b.txt", "This is doc/b.txt.\n"},
        {"root/doc/c.txt", "This is doc/c.txt.\n"},
        {"root/doc/d.txt", "This is doc/d.txt.\n"},
        {"root/doc/src/r.txt", "This is doc/src/r.txt.\n"},
        {"root/doc/src/s.txt", "This is doc/src/s.txt.\n"},
        {"root/doc/src/t.txt", "This is doc/src/t.txt.\n"},
        {"root/doc/src/u.txt", "This is doc/src/u.txt.\n"},
        {"root/other/x/y/z.txt", "other\n"},
        {"root/toplevel.txt", "toplevel\n"},
    });
  }

  std::unique_ptr<ScmStatus> diff(const RootId& id) {
    auto fut = testMount_.getEdenMount()
                   ->diff(
                       testMount_.getRootInode(),
                       id,
                       folly::CancellationToken{},
                       ObjectFetchContext::getNullContext(),
                       /*listIgnored=*/true,
                       /*enforceCurrentParent=*/false)
                   .semi()
                   .via(testMount_.getServerExecutor().get());
    testMount_.drainServerExecutor();
    return std::move(fut).get(10ms);
  }

  void addFile(const std::string& path, const std::string& contents) {
    builder_.setFile(path, contents);
  }

  void modifyAndInitialize() {
    testMount_.initialize(builder_, /*startReady=*/true);

    // Locally modify some of the files under doc/
    // We need to make the blobs ready in order to modify the inodes,
    // but mark them not ready again afterwards.
    builder_.getStoredBlob("root/doc/a.txt"_relpath);
    builder_.getStoredBlob("root/doc/b.txt"_relpath);
    builder_.getStoredBlob("root/doc/c.txt"_relpath);
    builder_.getStoredBlob("root/doc/d.txt"_relpath);
    testMount_.overwriteFile("root/doc/a.txt", "updated a.txt\n");
    testMount_.overwriteFile("root/doc/b.txt", "updated b.txt\n");
    testMount_.overwriteFile("root/doc/c.txt", "updated c.txt\n");
    testMount_.overwriteFile("root/doc/d.txt", "updated d.txt\n");
  }

  FakeTreeBuilder builder_;
  TestMount testMount_;
};

TEST_F(DiffTestNonMateralized, diff_modified_trees_top_level_not_materialized) {
  modifyAndInitialize();
  auto backingStore = testMount_.getBackingStore();
  auto builder2 = builder_.clone();
  builder2.replaceFile("root/src/r.txt", "src/r.txt has been updated.\n");
  builder2.replaceFile("root/src/s.txt", "src/s.txt has also been updated.\n");
  builder2.replaceFile("root/src/t.txt", "src/t.txt updated.\n");
  builder2.replaceFile("root/src/u.txt", "src/u.txt updated.\n");

  // Add tree2 to the backing store and create a commit pointing to it.
  auto rootTree2 = builder2.finalize(backingStore, /*setReady=*/true);
  auto commitId2 = testMount_.nextCommitId();
  auto* commit2 =
      backingStore->putCommit(commitId2, rootTree2->get().getObjectId());
  commit2->setReady();

  // Run the diff. This will enter store/Diff.cpp via root/src using
  // a ModifiedScmEntry
  auto result = diff(commitId2);

  EXPECT_THAT(
      *result->entries(),
      UnorderedElementsAre(
          std::make_pair("root/src/r.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/src/s.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/src/t.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/src/u.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/doc/a.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/doc/b.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/doc/c.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/doc/d.txt", ScmFileStatus::MODIFIED)));
}

TEST_F(
    DiffTestNonMateralized,
    diff_modified_trees_top_level_not_materialized_with_gitignore) {
  addFile("root/src/.gitignore", "test");
  addFile("root/src/test/t.txt", "This will be ignored\n");
  modifyAndInitialize();
  auto backingStore = testMount_.getBackingStore();
  auto builder2 = builder_.clone();

  builder2.replaceFile("root/src/r.txt", "src/r.txt has been updated.\n");
  builder2.replaceFile("root/src/s.txt", "src/s.txt has also been updated.\n");
  builder2.replaceFile("root/src/t.txt", "src/t.txt updated.\n");
  builder2.replaceFile("root/src/u.txt", "src/u.txt updated.\n");
  builder2.removeFile("root/src/test/t.txt");

  // Add tree2 to the backing store and create a commit pointing to it.
  auto rootTree2 = builder2.finalize(backingStore, /*setReady=*/true);
  auto commitId2 = testMount_.nextCommitId();
  auto* commit2 =
      backingStore->putCommit(commitId2, rootTree2->get().getObjectId());
  commit2->setReady();

  // Run the diff. This will enter store/Diff.cpp via root/src using
  // a ModifiedScmEntry
  auto result = diff(commitId2);

  EXPECT_THAT(
      *result->entries(),
      UnorderedElementsAre(
          std::make_pair("root/src/test/t.txt", ScmFileStatus::ADDED),
          std::make_pair("root/src/r.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/src/s.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/src/t.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/src/u.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/doc/a.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/doc/b.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/doc/c.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/doc/d.txt", ScmFileStatus::MODIFIED)));
}

TEST_F(DiffTestNonMateralized, diff_modified_trees_low_level_not_materialized) {
  modifyAndInitialize();
  auto backingStore = testMount_.getBackingStore();
  auto builder2 = builder_.clone();
  builder2.replaceFile(
      "root/doc/src/r.txt", "doc/src/r.txt has been updated.\n");
  builder2.replaceFile(
      "root/doc/src/s.txt", "doc/src/s.txt has also been updated.\n");
  builder2.replaceFile("root/doc/src/t.txt", "doc/src/t.txt updated.\n");
  builder2.replaceFile("root/doc/src/u.txt", "doc/src/u.txt updated.\n");

  // materialize the inode not the files
  testMount_.getInode("root/doc/src"_relpath);

  // Add tree2 to the backing store and create a commit pointing to it.
  auto rootTree2 = builder2.finalize(backingStore, /*setReady=*/true);
  auto commitId2 = testMount_.nextCommitId();
  auto* commit2 =
      backingStore->putCommit(commitId2, rootTree2->get().getObjectId());
  commit2->setReady();

  // Run the diff. This will enter store/Diff.cpp via root/doc/src using
  // diffTrees from a ModifiedDiffEntry
  auto result = diff(commitId2);

  EXPECT_THAT(
      *result->entries(),
      UnorderedElementsAre(
          std::make_pair("root/doc/src/r.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/doc/src/s.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/doc/src/t.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/doc/src/u.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/doc/a.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/doc/b.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/doc/c.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/doc/d.txt", ScmFileStatus::MODIFIED)));
}

TEST_F(
    DiffTestNonMateralized,
    diff_modified_trees_low_level_not_materialized_with_gitignore) {
  addFile("root/doc/src/.gitignore", "test");
  addFile("root/doc/src/test/t.txt", "This will be ignored.\n");
  modifyAndInitialize();
  auto backingStore = testMount_.getBackingStore();

  // remove some files from the commit that are in the working copy.
  auto builder2 = builder_.clone();
  builder2.replaceFile(
      "root/doc/src/r.txt", "doc/src/r.txt has been updated.\n");
  builder2.replaceFile(
      "root/doc/src/s.txt", "doc/src/s.txt has also been updated.\n");
  builder2.replaceFile("root/doc/src/t.txt", "doc/src/t.txt updated.\n");
  builder2.replaceFile("root/doc/src/u.txt", "doc/src/u.txt updated.\n");
  builder2.removeFile("root/doc/src/test/t.txt");

  // materialize the inode not the files
  testMount_.getInode("root/doc/src"_relpath);

  // Add tree2 to the backing store and create a commit pointing to it.
  auto rootTree2 = builder2.finalize(backingStore, /*setReady=*/true);
  auto commitId2 = testMount_.nextCommitId();
  auto* commit2 =
      backingStore->putCommit(commitId2, rootTree2->get().getObjectId());
  commit2->setReady();

  // Run the diff. This will enter store/Diff.cpp via root/doc/src using
  // diffTrees from a DeferredDiffEntry
  auto result = diff(commitId2);

  EXPECT_THAT(
      *result->entries(),
      UnorderedElementsAre(
          std::make_pair("root/doc/src/test/t.txt", ScmFileStatus::ADDED),
          std::make_pair("root/doc/src/r.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/doc/src/s.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/doc/src/t.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/doc/src/u.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/doc/a.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/doc/b.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/doc/c.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/doc/d.txt", ScmFileStatus::MODIFIED)));
}

TEST_F(DiffTestNonMateralized, diff_added_tree_top_level_not_materialized) {
  modifyAndInitialize();
  auto backingStore = testMount_.getBackingStore();

  // remove some files from the commit that are in the working copy.
  auto builder2 = builder_.clone();
  builder2.removeFile("root/src/r.txt");
  builder2.removeFile("root/src/s.txt");
  builder2.removeFile("root/src/t.txt");
  builder2.removeFile("root/src/u.txt");

  // Add tree2 to the backing store and create a commit pointing to it.
  auto rootTree2 = builder2.finalize(backingStore, /*setReady=*/true);
  auto commitId2 = testMount_.nextCommitId();
  auto* commit2 =
      backingStore->putCommit(commitId2, rootTree2->get().getObjectId());
  commit2->setReady();

  // Run the diff. This will enter store/Diff.cpp via root/src using
  // createAddedScmEntry
  auto result = diff(commitId2);

  EXPECT_THAT(
      *result->entries(),
      UnorderedElementsAre(
          std::make_pair("root/src/r.txt", ScmFileStatus::ADDED),
          std::make_pair("root/src/s.txt", ScmFileStatus::ADDED),
          std::make_pair("root/src/t.txt", ScmFileStatus::ADDED),
          std::make_pair("root/src/u.txt", ScmFileStatus::ADDED),
          std::make_pair("root/doc/a.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/doc/b.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/doc/c.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/doc/d.txt", ScmFileStatus::MODIFIED)));
}

TEST_F(
    DiffTestNonMateralized,
    diff_added_tree_top_level_not_materialized_with_gitignore) {
  addFile("root/.gitignore", "src/foo/r.txt");
  addFile("root/src/foo/r.txt", "test");
  modifyAndInitialize();
  auto backingStore = testMount_.getBackingStore();

  // remove some files from the commit that are in the working copy.
  auto builder2 = builder_.clone();
  builder2.removeFile("root/src/r.txt");
  builder2.removeFile("root/src/s.txt");
  builder2.removeFile("root/src/t.txt");
  builder2.removeFile("root/src/u.txt");
  builder2.removeFile("root/src/foo/r.txt");

  // Add tree2 to the backing store and create a commit pointing to it.
  auto rootTree2 = builder2.finalize(backingStore, /*setReady=*/true);
  auto commitId2 = testMount_.nextCommitId();
  auto* commit2 =
      backingStore->putCommit(commitId2, rootTree2->get().getObjectId());
  commit2->setReady();

  // Run the diff. This will enter store/Diff.cpp via root/src using
  // createAddedScmEntry
  auto result = diff(commitId2);

  EXPECT_THAT(
      *result->entries(),
      UnorderedElementsAre(
          std::make_pair("root/src/foo/r.txt", ScmFileStatus::ADDED),
          std::make_pair("root/src/r.txt", ScmFileStatus::ADDED),
          std::make_pair("root/src/s.txt", ScmFileStatus::ADDED),
          std::make_pair("root/src/t.txt", ScmFileStatus::ADDED),
          std::make_pair("root/src/u.txt", ScmFileStatus::ADDED),
          std::make_pair("root/doc/a.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/doc/b.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/doc/c.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/doc/d.txt", ScmFileStatus::MODIFIED)));
}

TEST_F(DiffTestNonMateralized, diff_removed_tree_top_level_not_materialized) {
  modifyAndInitialize();
  auto backingStore = testMount_.getBackingStore();

  // put some files in the commit that are not in the working copy.
  auto builder2 = builder_.clone();
  builder2.setFile("root/another/r.txt", "This is another/r.txt.\n");
  builder2.setFile("root/another/s.txt", "This is another/s.txt.\n");
  builder2.setFile("root/another/t.txt", "This is another/t.txt.\n");
  builder2.setFile("root/another/u.txt", "This is another/u.txt.\n");

  // Add tree2 to the backing store and create a commit pointing to it.
  auto rootTree2 = builder2.finalize(backingStore, /*setReady=*/true);
  auto commitId2 = testMount_.nextCommitId();
  auto* commit2 =
      backingStore->putCommit(commitId2, rootTree2->get().getObjectId());
  commit2->setReady();

  // Run the diff. This will enter store/Diff.cpp via root/src using
  // RemovedScmEntry via processRemoved
  auto result = diff(commitId2);

  EXPECT_THAT(
      *result->entries(),
      UnorderedElementsAre(
          std::make_pair("root/another/r.txt", ScmFileStatus::REMOVED),
          std::make_pair("root/another/s.txt", ScmFileStatus::REMOVED),
          std::make_pair("root/another/t.txt", ScmFileStatus::REMOVED),
          std::make_pair("root/another/u.txt", ScmFileStatus::REMOVED),
          std::make_pair("root/doc/a.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/doc/b.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/doc/c.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/doc/d.txt", ScmFileStatus::MODIFIED)));
}

TEST_F(
    DiffTestNonMateralized,
    diff_top_level_tree_replaced_with_file_not_materialized) {
  addFile("root/another", "replaced with file\n");
  modifyAndInitialize();
  auto backingStore = testMount_.getBackingStore();

  // put some files in the commit that are not in the working copy.
  auto builder2 = builder_.clone();
  builder2.removeFile("root/another");
  builder2.setFile("root/another/r.txt", "This is another/r.txt.\n");
  builder2.setFile("root/another/s.txt", "This is another/s.txt.\n");
  builder2.setFile("root/another/t.txt", "This is another/t.txt.\n");
  builder2.setFile("root/another/u.txt", "This is sranotherc/u.txt.\n");

  // Add tree2 to the backing store and create a commit pointing to it.
  auto rootTree2 = builder2.finalize(backingStore, /*setReady=*/true);
  auto commitId2 = testMount_.nextCommitId();
  auto* commit2 =
      backingStore->putCommit(commitId2, rootTree2->get().getObjectId());
  commit2->setReady();

  // Run the diff. This will enter store/Diff.cpp via root/src using
  // RemovedScmEntry via processBothPresent
  auto result = diff(commitId2);

  EXPECT_THAT(
      *result->entries(),
      UnorderedElementsAre(
          std::make_pair("root/another/r.txt", ScmFileStatus::REMOVED),
          std::make_pair("root/another/s.txt", ScmFileStatus::REMOVED),
          std::make_pair("root/another/t.txt", ScmFileStatus::REMOVED),
          std::make_pair("root/another/u.txt", ScmFileStatus::REMOVED),
          std::make_pair("root/another", ScmFileStatus::ADDED),
          std::make_pair("root/doc/a.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/doc/b.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/doc/c.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/doc/d.txt", ScmFileStatus::MODIFIED)));
}

TEST_F(DiffTestNonMateralized, diff_removed_tree_low_level_not_materialized) {
  addFile("root/doc/another", "This is another\n");
  modifyAndInitialize();
  auto backingStore = testMount_.getBackingStore();

  // remove some files from the commit that are in the working copy.
  auto builder2 = builder_.clone();
  builder2.removeFile("root/doc/another");
  builder2.setFile("root/doc/another/r.txt", "This is another/r.txt.\n");
  builder2.setFile("root/doc/another/s.txt", "This is another/s.txt.\n");
  builder2.setFile("root/doc/another/t.txt", "This is another/t.txt.\n");
  builder2.setFile("root/doc/another/u.txt", "This is another/u.txt.\n");

  // materialize the inode not the files
  testMount_.getInode("root/doc/another"_relpath);

  // Add tree2 to the backing store and create a commit pointing to it.
  auto rootTree2 = builder2.finalize(backingStore, /*setReady=*/true);
  auto commitId2 = testMount_.nextCommitId();
  auto* commit2 =
      backingStore->putCommit(commitId2, rootTree2->get().getObjectId());
  commit2->setReady();

  // Run the diff. This will enter store/Diff.cpp via root/doc/src using
  // diffRemovedTree from a ModifiedDiffEntry
  auto result = diff(commitId2);

  EXPECT_THAT(
      *result->entries(),
      UnorderedElementsAre(
          std::make_pair("root/doc/another/r.txt", ScmFileStatus::REMOVED),
          std::make_pair("root/doc/another/s.txt", ScmFileStatus::REMOVED),
          std::make_pair("root/doc/another/t.txt", ScmFileStatus::REMOVED),
          std::make_pair("root/doc/another/u.txt", ScmFileStatus::REMOVED),
          std::make_pair("root/doc/another", ScmFileStatus::ADDED),
          std::make_pair("root/doc/a.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/doc/b.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/doc/c.txt", ScmFileStatus::MODIFIED),
          std::make_pair("root/doc/d.txt", ScmFileStatus::MODIFIED)));
}

TEST(DiffTest, multiTreeDiff) {
  TestMount testMount;

  auto builder1 = FakeTreeBuilder();
  builder1.setFile("a", "A in 1\n");
  builder1.setFile("b", "B in 1\n");
  builder1.setFile("c", "C in 1\n");
  auto rootTree1 = builder1.finalize(testMount.getBackingStore(), true);
  auto commit1 = testMount.getBackingStore()->putCommit("1", builder1);
  commit1->setReady();

  auto builder2 = builder1.clone();
  builder2.replaceFile("a", "A in 2\n");
  builder2.removeFile("c");
  builder2.setFile("d", "D in 2\n");
  auto rootTree2 = builder2.finalize(testMount.getBackingStore(), true);
  auto commit2 = testMount.getBackingStore()->putCommit("2", builder2);
  commit2->setReady();

  // Checkout commit #2
  testMount.initialize(RootId("2"));

  const auto& edenMount = testMount.getEdenMount();

  auto root = edenMount->getRootInode();

  ScmStatusDiffCallback callback;
  DiffContext diffContext{
      &callback,
      folly::CancellationToken{},
      ObjectFetchContext::getNullContext(),
      false, // listIgnored
      kPathMapDefaultCaseSensitive,
      true,
      testMount.getEdenMount()->getObjectStore(),
      std::make_unique<TopLevelIgnores>("", "")};

  // Modify "a" to match commit 1, even though we're on commit 2.
  testMount.overwriteFile("a", "A in 1\n");

  // Modify "b" to match neither commit 1 nor commit 2.
  testMount.overwriteFile("b", "B in 3\n");

  // Test diff against no trees
  std::vector<std::shared_ptr<const Tree>> trees;
  root->diff(
          &diffContext,
          RelativePathPiece{},
          std::move(trees),
          diffContext.getToplevelIgnore(),
          false)
      .get(0ms);

  auto status = callback.extractStatus();
  EXPECT_THAT(*status.errors(), UnorderedElementsAre());
  EXPECT_THAT(
      *status.entries(),
      UnorderedElementsAre(
          std::make_pair("a", ScmFileStatus::ADDED),
          std::make_pair("b", ScmFileStatus::ADDED),
          std::make_pair("d", ScmFileStatus::ADDED)));

  // Test diff against commit1
  callback = ScmStatusDiffCallback();
  trees = {std::make_shared<const Tree>(rootTree1->get())};
  root->diff(
          &diffContext,
          RelativePathPiece{},
          std::move(trees),
          diffContext.getToplevelIgnore(),
          false)
      .get(0ms);

  status = callback.extractStatus();
  EXPECT_THAT(*status.errors(), UnorderedElementsAre());
  EXPECT_THAT(
      *status.entries(),
      UnorderedElementsAre(
          std::make_pair("b", ScmFileStatus::MODIFIED),
          std::make_pair("c", ScmFileStatus::REMOVED),
          std::make_pair("d", ScmFileStatus::ADDED)));

  // Test diff against commit2
  callback = ScmStatusDiffCallback();
  trees = {std::make_shared<const Tree>(rootTree2->get())};
  root->diff(
          &diffContext,
          RelativePathPiece{},
          std::move(trees),
          diffContext.getToplevelIgnore(),
          false)
      .get(0ms);

  status = callback.extractStatus();
  EXPECT_THAT(*status.errors(), UnorderedElementsAre());
  EXPECT_THAT(
      *status.entries(),
      UnorderedElementsAre(
          std::make_pair("a", ScmFileStatus::MODIFIED),
          std::make_pair("b", ScmFileStatus::MODIFIED)));

  // Test diff against commit1 and commit2
  callback = ScmStatusDiffCallback();
  trees = {
      std::make_shared<const Tree>(rootTree1->get()),
      std::make_shared<const Tree>(rootTree2->get())};
  root->diff(
          &diffContext,
          RelativePathPiece{},
          std::move(trees),
          diffContext.getToplevelIgnore(),
          false)
      .get(0ms);

  status = callback.extractStatus();
  EXPECT_THAT(*status.errors(), UnorderedElementsAre());
  EXPECT_THAT(
      *status.entries(),
      UnorderedElementsAre(std::make_pair("b", ScmFileStatus::MODIFIED)));
}
