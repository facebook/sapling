/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/test/TestUtils.h>
#include <gmock/gmock.h>
#include <gtest/gtest.h>
#include <filesystem>
#include <iostream>
#include <memory>
#include <string>
#include "eden/fs/model/Hash.h"
#include "eden/fs/store/DiffContext.h"
#include "eden/fs/store/ScmStatusDiffCallback.h"
#include "eden/fs/testharness/FakeBackingStore.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestUtil.h"
#include "eden/fs/win/mount/CurrentState.h"
#include "eden/fs/win/mount/GenerateStatus.h"
#include "eden/fs/win/mount/StateDirectoryEntry.h"
#include "eden/fs/win/testharness/TestMount.h"
#include "eden/fs/win/utils/FileUtils.h"
#include "eden/fs/win/utils/Guid.h"
#include "eden/fs/win/utils/StringConv.h"

using namespace facebook::eden;
using namespace std::filesystem;
using testing::UnorderedElementsAre;

// TODO(puneetk): This is a duplicate function between this code and difftest.
// We should create a common function and use it at both the places.
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

struct StatusTest {
  StatusTest() {
    // Set up a directory structure that we will use for most
    // of the tests below
    builder_.setFiles({
        {"src/1.txt", "This is src/1.txt.\n"},
        {"src/2.txt", "This is src/2.txt.\n"},
        {"src/a/b/3.txt", "This is 3.txt.\n"},
        {"src/a/b/c/4.txt", "This is 4.txt.\n"},
        {"doc/readme.txt", "No one reads docs.\n"},
        {"toplevel.txt", "toplevel\n"},
    });
    testMount_.initialize(builder_);
  }

  explicit StatusTest(
      std::initializer_list<FakeTreeBuilder::FileInfo>&& fileArgs) {
    builder_.setFiles(std::move(fileArgs));
    testMount_.initialize(builder_);
  }

  explicit StatusTest(FakeTreeBuilder&& builder)
      : builder_{std::move(builder)} {
    testMount_.initialize(builder_);
  }

  folly::Future<std::unique_ptr<ScmStatus>> getStatusFuture(
      bool listIgnored = false) {
    auto commitHash = testMount().getEdenMount()->getParentCommits().parent1();
    return (testMount().getMount()->diff(
        commitHash,
        /*listIgnored=*/false,
        /*enforceCurrentParent=*/false,
        /*ResponseChannelRequest* =*/nullptr));
  }

  std::unique_ptr<ScmStatus> getStatus(bool listIgnored = false) {
    return getStatusFuture(listIgnored).get();
  }

  void checkNoChanges() {
    auto result = getStatus();
    EXPECT_THAT(result->entries, testing::UnorderedElementsAre());
  }

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
  std::unique_ptr<ScmStatus> resetCommitAndDiff(FakeTreeBuilder& builder) {
    testMount_.resetCommit(builder, /* setReady = */ true);
    auto df = getStatusFuture();
    return EXPECT_FUTURE_RESULT(df);
  }

  TestMount& testMount() {
    return testMount_;
  }

  FakeTreeBuilder& getBuilder() {
    return builder_;
  }

  TestMount testMount_;
  FakeTreeBuilder builder_;
};

TEST(StatusTest, emptyClone) {
  FakeTreeBuilder builder;
  builder.setFile("a/b/c/d/e/file1.txt", "file1 contents");
  builder.setFile("a/b/file2.txt", "file 2 contents");
  builder.setFile("a/b/file3.txt", "file 3 contents");
  builder.setFile("hh/bb/cc/dd/ee/file1.cpp", "file1 contents");
  builder.setFile("hh/bb/cc/dd/ee/file2.h", "file1 contents");
  builder.setFile("hh/bb/cc/file3.cpp", "file 2 contents");
  builder.setFile("hh/bb/cc/file4.cpp", "file 3 contents");

  StatusTest statusTest(std::move(builder));
  EXPECT_EQ(statusTest.getStatus()->entries.size(), 0);
}

TEST(StatusTest, basicStatusTests) {
  FakeTreeBuilder builder;
  builder.setFile("a/b/c/d/e/file1.txt", "file1 contents");
  builder.setFile("a/b/file2.txt", "file 2 contents");
  builder.setFile("a/b/file3.txt", "file 3 contents");
  builder.setFile("hh/bb/cc/dd/ee/file1.cpp", "file1 contents");
  builder.setFile("hh/bb/cc/dd/ee/file2.h", "file1 contents");
  builder.setFile("hh/bb/cc/file3.cpp", "file 2 contents");
  builder.setFile("hh/bb/cc/file4.cpp", "file 3 contents");

  StatusTest statusTest(std::move(builder));
  TestMount& mount = statusTest.testMount();

  mount.createEntry(L"a", /* isDirectory=*/true, "a");
  mount.createEntry(L"a\\b", /* isDirectory=*/true, "b");
  mount.createEntry(L"a\\b\\c", /* isDirectory=*/true, "c");
  mount.createEntry(L"a\\b\\c\\d", /* isDirectory=*/true, "d");
  mount.createEntry(L"a\\b\\c\\d\\e", /* isDirectory=*/true, "e");

  mount.createEntry(
      L"a\\b\\c\\d\\e\\file1.txt", /* isDirectory=*/false, "ffff1");

  EXPECT_EQ(statusTest.getStatus()->entries.size(), 0);

  mount.loadEntry(L"a\\b\\c\\d\\e\\file1.txt");
  EXPECT_EQ(statusTest.getStatus()->entries.size(), 0);

  // Create a new folder
  mount.createDirectory(L"a\\b\\c\\d\\f");
  EXPECT_EQ(statusTest.getStatus()->entries.size(), 0);

  std::map<PathString, ScmFileStatus> expectedStatus;

  // Create a new file
  mount.createFile(L"a\\b\\c\\d\\newfile1.toml", "New file text");
  expectedStatus["a/b/c/d/newfile1.toml"] = ScmFileStatus::ADDED;
  EXPECT_EQ(statusTest.getStatus()->entries, expectedStatus);

  // Create new file at root
  mount.createFile(L"newfile2.toml", "New file text");

  expectedStatus["newfile2.toml"] = ScmFileStatus::ADDED;
  EXPECT_EQ(statusTest.getStatus()->entries, expectedStatus);

  // Create and modify "a/b/file2.txt" from SCM
  mount.createEntry(L"a\\b\\file2.txt", /* isDirectory=*/false, "ffff2");
  EXPECT_EQ(statusTest.getStatus()->entries, expectedStatus);

  mount.loadEntry(L"a\\b\\file2.txt");
  EXPECT_EQ(statusTest.getStatus()->entries, expectedStatus);

  mount.modifyFile(L"a\\b\\file2.txt", "file text");
  expectedStatus["a/b/file2.txt"] = ScmFileStatus::MODIFIED;
  EXPECT_EQ(statusTest.getStatus()->entries, expectedStatus);

  // Set the contents of the file to match the SCM
  mount.modifyFile(L"a\\b\\file2.txt", "file 2 contents");
  expectedStatus.erase("a/b/file2.txt");
  EXPECT_EQ(statusTest.getStatus()->entries, expectedStatus);

  // Modify the file contents again
  mount.modifyFile(L"a\\b\\file2.txt", "file 2 modified contents");
  expectedStatus["a/b/file2.txt"] = ScmFileStatus::MODIFIED;
  EXPECT_EQ(statusTest.getStatus()->entries, expectedStatus);

  // Delete the modified file
  mount.removeFile("a\\b\\file2.txt");
  expectedStatus["a/b/file2.txt"] = ScmFileStatus::REMOVED;
  EXPECT_EQ(statusTest.getStatus()->entries, expectedStatus);

  // Recreate the deleted the file
  mount.createFile(L"a\\b\\file2.txt", "default text");
  expectedStatus["a/b/file2.txt"] = ScmFileStatus::MODIFIED;
  EXPECT_EQ(statusTest.getStatus()->entries, expectedStatus);

  // Deleting and recreating a file with same contents should not change
  mount.removeFile(L"a\\b\\c\\d\\e\\file1.txt");
  mount.createFile(L"a\\b\\c\\d\\e\\file1.txt", "file1 contents");

  EXPECT_EQ(statusTest.getStatus()->entries, expectedStatus);

  // Remove it again
  mount.removeFile(L"a\\b\\c\\d\\e\\file1.txt");
  expectedStatus["a/b/c/d/e/file1.txt"] = ScmFileStatus::REMOVED;
  EXPECT_EQ(statusTest.getStatus()->entries, expectedStatus);
}

TEST(StatusTest, removeSubTree) {
  FakeTreeBuilder builder;
  builder.setFile("aa/bb/cc/dd/ee/file1.txt", "file1 contents");
  builder.setFile("aa/bb/cc/dd/ee/file2.txt", "file2 contents");
  builder.setFile("aa/bb/cc/file3.txt", "file 3 contents");
  builder.setFile("aa/bb/cc/file4.txt", "file 4 contents");

  builder.setFile("hh/bb/cc/dd/ee/file1.cpp", "file1 contents");
  builder.setFile("hh/bb/cc/dd/ee/file2.h", "file1 contents");
  builder.setFile("hh/bb/cc/file3.cpp", "file 2 contents");
  builder.setFile("hh/bb/cc/file4.cpp", "file 3 contents");

  StatusTest statusTest(std::move(builder));
  TestMount& mount = statusTest.testMount();

  mount.createEntry(L"aa", /*isDirectory=*/true, "a");
  mount.createEntry(L"aa\\bb", /*isDirectory=*/true, "b");
  mount.createEntry(L"aa\\bb\\cc", /*isDirectory=*/true, "c");
  mount.createEntry(L"aa\\bb\\cc\\dd", /*isDirectory=*/true, "d");
  mount.createEntry(L"aa\\bb\\cc\\dd\\ee", /*isDirectory=*/true, "e");
  mount.createEntry(
      L"aa\\bb\\cc\\dd\\ee\\file1.txt", /*isDirectory=*/false, "ffff1");
  mount.createEntry(
      L"aa\\bb\\cc\\dd\\ee\\file2.txt", /*isDirectory=*/false, "ffff2");
  mount.createEntry(L"aa\\bb\\cc\\file3.txt", /*isDirectory=*/false, "ffff3");
  mount.createEntry(L"aa\\bb\\cc\\file4.txt", /*isDirectory=*/false, "ffff4");

  std::map<PathString, ScmFileStatus> expectedStatus;

  //
  // Delete the directory "aa\\bb\\cc". On the real file system, the
  //  directories are always deleted after recursively deleting all the sub
  //  files and folders. We will simulate that here.
  //
  mount.removeFile(L"aa\\bb\\cc\\dd\\ee\\file1.txt");
  mount.removeFile(L"aa\\bb\\cc\\dd\\ee\\file2.txt");
  mount.removeFile(L"aa\\bb\\cc\\file3.txt");
  mount.removeFile(L"aa\\bb\\cc\\file4.txt");
  mount.removeDirectory(L"aa\\bb\\cc\\dd\\ee");
  mount.removeDirectory(L"aa\\bb\\cc\\dd");
  mount.removeDirectory(L"aa\\bb\\cc");

  // We expect the following files removed.
  expectedStatus["aa/bb/cc/dd/ee/file1.txt"] = ScmFileStatus::REMOVED;
  expectedStatus["aa/bb/cc/dd/ee/file2.txt"] = ScmFileStatus::REMOVED;
  expectedStatus["aa/bb/cc/file3.txt"] = ScmFileStatus::REMOVED;
  expectedStatus["aa/bb/cc/file4.txt"] = ScmFileStatus::REMOVED;
  EXPECT_EQ(statusTest.getStatus()->entries, expectedStatus);

  // Create a file in its place
  mount.createFile(L"aa\\bb\\cc", "something");
  expectedStatus["aa/bb/cc"] = ScmFileStatus::ADDED;
  EXPECT_EQ(statusTest.getStatus()->entries, expectedStatus);

  // Delete the file and create a folder back again
  mount.removeFile(L"aa\\bb\\cc");
  expectedStatus.erase("aa/bb/cc");
  EXPECT_EQ(statusTest.getStatus()->entries, expectedStatus);

  //  // Creating a folder - this should not change the status
  mount.createDirectory(L"aa\\bb\\cc");
  EXPECT_EQ(statusTest.getStatus()->entries, expectedStatus);

  // Adding a file inside it should report in the status
  mount.createFile(L"aa\\bb\\cc\\file3.txt", "text");
  expectedStatus["aa/bb/cc/file3.txt"] = ScmFileStatus::MODIFIED;
  EXPECT_EQ(statusTest.getStatus()->entries, expectedStatus);
}

TEST(StatusTest, fileModified) {
  StatusTest test;
  TestMount& mount = test.testMount();
  mount.createEntry(L"src", /*isDirectory=*/true, "1");
  mount.createEntry(L"src\\1.txt", /*isDirectory=*/false, "1");
  mount.loadEntry(L"src\\1.txt");
  mount.modifyFile(L"src\\1.txt", "This file has been updated.\n");

  EXPECT_THAT(
      test.getStatus()->entries,
      UnorderedElementsAre(
          std::make_pair("src/1.txt", ScmFileStatus::MODIFIED)));
}

TEST(StatusTest, fileRemoved) {
  StatusTest test;
  TestMount& mount = test.testMount();

  mount.createEntry(L"src", /*isDirectory=*/true, "1");
  mount.createEntry(L"src\\1.txt", /*isDirectory=*/false, "1");
  mount.loadEntry(L"src\\1.txt");
  mount.removeFile(L"src\\1.txt");

  EXPECT_THAT(
      test.getStatus()->entries,
      UnorderedElementsAre(
          std::make_pair("src/1.txt", ScmFileStatus::REMOVED)));
}

TEST(StatusTest, fileAdded) {
  StatusTest test;
  TestMount& mount = test.testMount();
  mount.createEntry(L"src", /*isDirectory=*/true, "1");
  mount.createFile(L"src\\new.txt", "extra stuff");

  EXPECT_THAT(
      test.getStatus()->entries,
      UnorderedElementsAre(
          std::make_pair("src/new.txt", ScmFileStatus::ADDED)));
}

TEST(StatusTest, directoryRemoved) {
  StatusTest test;
  TestMount& mount = test.testMount();
  mount.createEntry(L"src", /*isDirectory=*/true, "1");
  mount.createEntry(L"src\\a", /*isDirectory=*/true, "1");
  mount.createEntry(L"src\\a\\b", /*isDirectory=*/true, "1");
  mount.createEntry(L"src\\a\\b\\c", /*isDirectory=*/true, "1");

  mount.createEntry(L"src\\a\\b\\3.txt", /*isDirectory=*/false, "1");
  mount.loadEntry(L"src\\a\\b\\3.txt");
  mount.removeFile(L"src\\a\\b\\3.txt");

  mount.createEntry(L"src\\a\\b\\c\\4.txt", /*isDirectory=*/false, "1");
  mount.loadEntry(L"src\\a\\b\\c\\4.txt");
  mount.removeFile(L"src\\a\\b\\c\\4.txt");

  mount.removeDirectory(L"src\\a\\b\\c");
  mount.removeDirectory(L"src\\a\\b");

  EXPECT_THAT(
      test.getStatus()->entries,
      UnorderedElementsAre(
          std::make_pair("src/a/b/c/4.txt", ScmFileStatus::REMOVED),
          std::make_pair("src/a/b/3.txt", ScmFileStatus::REMOVED)));
}

TEST(StatusTest, directoryAdded) {
  StatusTest test;
  TestMount& mount = test.testMount();
  mount.createEntry(L"src", /*isDirectory=*/true, "1");
  mount.createDirectory(L"src\\new");
  mount.createDirectory(L"src\\new\\subdir");
  mount.createFile(L"src\\new\\file.txt", "extra stuff");
  mount.createFile(L"src\\new\\subdir\\foo.txt", "extra stuff");
  mount.createFile(L"src\\new\\subdir\\bar.txt", "more extra stuff");

  EXPECT_THAT(
      test.getStatus()->entries,
      UnorderedElementsAre(
          std::make_pair("src/new/file.txt", ScmFileStatus::ADDED),
          std::make_pair("src/new/subdir/foo.txt", ScmFileStatus::ADDED),
          std::make_pair("src/new/subdir/bar.txt", ScmFileStatus::ADDED)));
}

TEST(StatusTest, dirReplacedWithFile) {
  StatusTest test;
  TestMount& mount = test.testMount();
  mount.createEntry(L"src", /*isDirectory=*/true, "1");
  mount.createEntry(L"src\\a", /*isDirectory=*/true, "1");
  mount.createEntry(L"src\\a\\b", /*isDirectory=*/true, "1");
  mount.createEntry(L"src\\a\\b\\c", /*isDirectory=*/true, "1");

  mount.createEntry(L"src\\a\\b\\3.txt", /*isDirectory=*/false, "1");
  mount.loadEntry(L"src\\a\\b\\3.txt");
  mount.removeFile(L"src\\a\\b\\3.txt");

  mount.createEntry(L"src\\a\\b\\c\\4.txt", /*isDirectory=*/false, "1");
  mount.loadEntry(L"src\\a\\b\\c\\4.txt");
  mount.removeFile(L"src\\a\\b\\c\\4.txt");

  mount.removeDirectory(L"src\\a\\b\\c");
  mount.removeDirectory(L"src\\a\\b");
  mount.createFile(L"src\\a\\b", "this is now a file");

  EXPECT_THAT(
      test.getStatus()->entries,
      UnorderedElementsAre(
          std::make_pair("src/a/b", ScmFileStatus::ADDED),
          std::make_pair("src/a/b/3.txt", ScmFileStatus::REMOVED),
          std::make_pair("src/a/b/c/4.txt", ScmFileStatus::REMOVED)));
}

TEST(StatusTest, fileReplacedWithDir) {
  StatusTest test;
  TestMount& mount = test.testMount();
  mount.createEntry(L"src", /*isDirectory=*/true, "1");

  mount.createEntry(L"src\\2.txt", /*isDirectory=*/false, "1");
  mount.loadEntry(L"src\\2.txt");
  mount.removeFile(L"src\\2.txt");

  mount.createDirectory(L"src\\2.txt");
  mount.createDirectory(L"src\\2.txt\\subdir");
  mount.createFile(L"src\\2.txt\\file.txt", "extra stuff");
  mount.createFile(L"src\\2.txt\\subdir\\foo.txt", "extra stuff");
  mount.createFile(L"src\\2.txt\\subdir\\bar.txt", "more extra stuff");

  EXPECT_THAT(
      test.getStatus()->entries,
      UnorderedElementsAre(
          std::make_pair("src/2.txt/file.txt", ScmFileStatus::ADDED),
          std::make_pair("src/2.txt/subdir/foo.txt", ScmFileStatus::ADDED),
          std::make_pair("src/2.txt/subdir/bar.txt", ScmFileStatus::ADDED),
          std::make_pair("src/2.txt", ScmFileStatus::REMOVED)));
}

// Test file adds/removes/modifications with various orderings of names between
// the fs entries and Tree entries.  This exercises the code that walks
// through the two entry lists comparing entry names.
TEST(StatusTest, pathOrdering) {
  StatusTest test({
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
  TestMount& mount = test.testMount();
  mount.createEntry(L"one", /*isDirectory=*/true, "1");
  mount.createEntry(L"two", /*isDirectory=*/true, "1");
  mount.createEntry(L"three", /*isDirectory=*/true, "1");

  mount.createFile(L"one\\aaa.txt", "test");
  // Add a file in the middle of the two entries in the source control Tree
  mount.createFile(L"one\\mmm.txt", "test");
  mount.createFile(L"one\\zzz.txt", "test");

  // In directory two, remove the opposite entries, so that the source control
  // Tree has the first and last entries.
  mount.createEntry(L"two\\aaa.txt", /*isDirectory=*/false, "1");
  mount.createEntry(L"two\\mmm.txt", /*isDirectory=*/false, "1");
  mount.createEntry(L"two\\zzz.txt", /*isDirectory=*/false, "1");

  mount.loadEntry(L"two\\aaa.txt");
  mount.loadEntry(L"two\\mmm.txt");
  mount.loadEntry(L"two\\zzz.txt");

  mount.removeFile(L"two\\aaa.txt");
  mount.removeFile(L"two\\mmm.txt");
  mount.removeFile(L"two\\zzz.txt");

  // In directory three, overwrite these 3 entries, so that the first and last
  // files are modified, plus one in the middle.
  mount.createEntry(L"three\\aaa.txt", /*isDirectory=*/false, "1");
  mount.createEntry(L"three\\mmm.txt", /*isDirectory=*/false, "1");
  mount.createEntry(L"three\\zzz.txt", /*isDirectory=*/false, "1");

  mount.loadEntry(L"three\\aaa.txt");
  mount.loadEntry(L"three\\mmm.txt");
  mount.loadEntry(L"three\\zzz.txt");

  mount.modifyFile(L"three\\aaa.txt", "updated contents\n");
  mount.modifyFile(L"three\\mmm.txt", "updated contents\n");
  mount.modifyFile(L"three\\zzz.txt", "updated contents\n");

  EXPECT_THAT(
      test.getStatus()->entries,
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

// TODO(puneetk): The following test is not yet working on Windows. Commenting
// out the test instead of removing because we need to make status work to fix
// the following test and test it with the following code.

// void testResetFileModified() {
//  StatusTest t;
//  auto b2 = t.getBuilder().clone();
//  b2.replaceFile("src/1.txt", "This file has been updated.\n");
//
//  auto result = t.resetCommitAndDiff(b2);
//  EXPECT_THAT(
//      result->entries,
//      UnorderedElementsAre(
//          std::make_pair("src/1.txt", ScmFileStatus::MODIFIED)));
//}
//
// TEST(StatusTest, resetFileModified) {
//  testResetFileModified();
//}
