/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */

#include <gtest/gtest.h>
#include "eden/fs/inodes/Dirstate.h"
#include "eden/fs/testharness/TestMount.h"

using namespace facebook::eden;

TEST(HgStatus, toString) {
  std::unordered_map<RelativePath, HgStatusCode> statuses({{
      {RelativePath("clean.txt"), HgStatusCode::CLEAN},
      {RelativePath("modified.txt"), HgStatusCode::MODIFIED},
      {RelativePath("added.txt"), HgStatusCode::ADDED},
      {RelativePath("removed.txt"), HgStatusCode::REMOVED},
      {RelativePath("missing.txt"), HgStatusCode::MISSING},
      {RelativePath("not_tracked.txt"), HgStatusCode::NOT_TRACKED},
      {RelativePath("ignored.txt"), HgStatusCode::IGNORED},
  }});
  HgStatus hgStatus(std::move(statuses));
  EXPECT_EQ(
      "A added.txt\n"
      "C clean.txt\n"
      "I ignored.txt\n"
      "! missing.txt\n"
      "M modified.txt\n"
      "? not_tracked.txt\n"
      "R removed.txt\n",
      hgStatus.toString());
}

void verifyExpectedDirstate(
    const Dirstate* dirstate,
    std::unordered_map<std::string, HgStatusCode>&& statuses) {
  std::unordered_map<RelativePath, HgStatusCode> expected;
  for (auto& pair : statuses) {
    expected.emplace(RelativePath(pair.first), pair.second);
  }
  auto expectedStatus = HgStatus(std::move(expected));
  EXPECT_EQ(expectedStatus, *dirstate->getStatus().get());
}

void verifyEmptyDirstate(const Dirstate* dirstate) {
  auto status = dirstate->getStatus();
  EXPECT_EQ(0, status->size()) << "Expected dirstate to be empty.";
}

/**
 * Calls `dirstate->removeAll({path}, force, errorsToReport)` and fails if
 * errorsToReport is non-empty.
 */
void scmRemoveFile(Dirstate* dirstate, std::string path, bool force) {
  std::vector<DirstateRemoveError> errorsToReport;
  std::vector<RelativePathPiece> paths({RelativePathPiece(path)});
  dirstate->removeAll(&paths, force, errorsToReport);
  if (!errorsToReport.empty()) {
    FAIL() << "Unexpected error: " << errorsToReport[0];
  }
}

/**
 * Calls `dirstate->removeAll({path}, force, errorsToReport)` and fails if
 * errorsToReport is not {expectedError}.
 */
void scmRemoveFileAndExpect(
    Dirstate* dirstate,
    std::string path,
    bool force,
    DirstateRemoveError expectedError) {
  std::vector<DirstateRemoveError> errorsToReport;
  std::vector<RelativePathPiece> paths({RelativePathPiece(path)});
  dirstate->removeAll(&paths, force, errorsToReport);
  std::vector<DirstateRemoveError> expectedErrors({expectedError});
  EXPECT_EQ(expectedErrors, errorsToReport);
}

TEST(Dirstate, createDirstate) {
  TestMountBuilder builder;
  auto testMount = builder.build();

  auto dirstate = testMount->getDirstate();
  verifyEmptyDirstate(dirstate);
}

TEST(Dirstate, createDirstateWithInitialState) {
  TestMountBuilder builder;
  builder.addFile({"removed.txt", "nada"});
  builder.addUserDirectives({
      {RelativePath("deleted.txt"), overlay::UserStatusDirective::Remove},
      {RelativePath("missing.txt"), overlay::UserStatusDirective::Add},
      {RelativePath("newfile.txt"), overlay::UserStatusDirective::Add},
      {RelativePath("removed.txt"), overlay::UserStatusDirective::Remove},
  });
  auto testMount = builder.build();
  testMount->addFile("newfile.txt", "legitimate add");

  auto dirstate = testMount->getDirstate();
  verifyExpectedDirstate(
      dirstate,
      {
          {"deleted.txt", HgStatusCode::REMOVED},
          {"missing.txt", HgStatusCode::MISSING},
          {"newfile.txt", HgStatusCode::ADDED},
          {"removed.txt", HgStatusCode::REMOVED},
      });
}

TEST(Dirstate, createDirstateWithUntrackedFile) {
  TestMountBuilder builder;
  auto testMount = builder.build();
  auto dirstate = testMount->getDirstate();

  testMount->addFile("hello.txt", "some contents");
  verifyExpectedDirstate(dirstate, {{"hello.txt", HgStatusCode::NOT_TRACKED}});
}

TEST(Dirstate, shouldIgnoreFilesInHgDirectory) {
  TestMountBuilder builder;
  auto testMount = builder.build();
  auto dirstate = testMount->getDirstate();

  testMount->mkdir(".hg");
  testMount->addFile(".hg/a-file", "contents");
  testMount->mkdir(".hg/some-extension");
  testMount->addFile(".hg/some-extension/a-file", "contents");
  testMount->mkdir(".hg/some-extension/with-a-directory");
  testMount->addFile(".hg/some-extension/with-a-directory/a-file", "contents");
  verifyEmptyDirstate(dirstate);
}

TEST(Dirstate, createDirstateWithAddedFile) {
  TestMountBuilder builder;
  auto testMount = builder.build();
  auto dirstate = testMount->getDirstate();

  testMount->addFile("hello.txt", "some contents");
  dirstate->add(RelativePathPiece("hello.txt"));
  verifyExpectedDirstate(dirstate, {{"hello.txt", HgStatusCode::ADDED}});
}

TEST(Dirstate, createDirstateWithMissingFile) {
  TestMountBuilder builder;
  auto testMount = builder.build();
  auto dirstate = testMount->getDirstate();

  testMount->addFile("hello.txt", "some contents");
  dirstate->add(RelativePathPiece("hello.txt"));
  testMount->deleteFile("hello.txt");
  verifyExpectedDirstate(dirstate, {{"hello.txt", HgStatusCode::MISSING}});
}

TEST(Dirstate, createDirstateWithModifiedFileContents) {
  TestMountBuilder builder;
  builder.addFile({"hello.txt", "some contents"});
  auto testMount = builder.build();
  auto dirstate = testMount->getDirstate();

  testMount->overwriteFile("hello.txt", "other contents");
  verifyExpectedDirstate(dirstate, {{"hello.txt", HgStatusCode::MODIFIED}});
}

TEST(Dirstate, createDirstateWithTouchedFile) {
  TestMountBuilder builder;
  builder.addFile({"hello.txt", "some contents"});
  auto testMount = builder.build();
  auto dirstate = testMount->getDirstate();

  testMount->overwriteFile("hello.txt", "some contents");
  // Although the file has been written, it has not changed in any significant
  // way.
  verifyEmptyDirstate(dirstate);
}

TEST(Dirstate, createDirstateWithFileAndThenHgRemoveIt) {
  TestMountBuilder builder;
  builder.addFile({"hello.txt", "some contents"});
  auto testMount = builder.build();
  auto dirstate = testMount->getDirstate();

  scmRemoveFile(dirstate, "hello.txt", /* force */ false);
  EXPECT_FALSE(testMount->hasFileAt("hello.txt"));

  verifyExpectedDirstate(dirstate, {{"hello.txt", HgStatusCode::REMOVED}});
}

TEST(Dirstate, createDirstateWithFileRemoveItAndThenHgRemoveIt) {
  TestMountBuilder builder;
  builder.addFile({"hello.txt", "some contents"});
  auto testMount = builder.build();
  auto dirstate = testMount->getDirstate();

  testMount->deleteFile("hello.txt");
  scmRemoveFile(dirstate, "hello.txt", /* force */ false);

  verifyExpectedDirstate(dirstate, {{"hello.txt", HgStatusCode::REMOVED}});
}

TEST(Dirstate, createDirstateWithFileTouchItAndThenHgRemoveIt) {
  TestMountBuilder builder;
  builder.addFile({"hello.txt", "original contents"});
  auto testMount = builder.build();
  auto dirstate = testMount->getDirstate();

  testMount->overwriteFile("hello.txt", "some other contents");

  scmRemoveFileAndExpect(
      dirstate,
      "hello.txt",
      /* force */ false,
      DirstateRemoveError{RelativePath("hello.txt"),
                          "not removing hello.txt: file is modified "
                          "(use -f to force removal)"});

  testMount->overwriteFile("hello.txt", "original contents");
  scmRemoveFile(dirstate, "hello.txt", /* force */ false);
  EXPECT_FALSE(testMount->hasFileAt("hello.txt"));

  verifyExpectedDirstate(dirstate, {{"hello.txt", HgStatusCode::REMOVED}});
}

TEST(Dirstate, createDirstateWithFileModifyItAndThenHgForceRemoveIt) {
  TestMountBuilder builder;
  builder.addFile({"hello.txt", "original contents"});
  auto testMount = builder.build();
  auto dirstate = testMount->getDirstate();

  testMount->overwriteFile("hello.txt", "some other contents");
  scmRemoveFile(dirstate, "hello.txt", /* force */ true);
  EXPECT_FALSE(testMount->hasFileAt("hello.txt"));
  verifyExpectedDirstate(dirstate, {{"hello.txt", HgStatusCode::REMOVED}});
}

TEST(Dirstate, ensureSubsequentCallsToHgRemoveHaveNoEffect) {
  TestMountBuilder builder;
  builder.addFile({"hello.txt", "original contents"});
  auto testMount = builder.build();
  auto dirstate = testMount->getDirstate();

  scmRemoveFile(dirstate, "hello.txt", /* force */ false);
  EXPECT_FALSE(testMount->hasFileAt("hello.txt"));
  verifyExpectedDirstate(dirstate, {{"hello.txt", HgStatusCode::REMOVED}});

  // Calling `hg remove` again should have no effect and not throw any errors.
  scmRemoveFile(dirstate, "hello.txt", /* force */ false);
  EXPECT_FALSE(testMount->hasFileAt("hello.txt"));
  verifyExpectedDirstate(dirstate, {{"hello.txt", HgStatusCode::REMOVED}});

  // Even if we restore the file, it should still show up as removed in
  // `hg status`.
  testMount->addFile("hello.txt", "original contents");
  EXPECT_TRUE(testMount->hasFileAt("hello.txt"));
  verifyExpectedDirstate(dirstate, {{"hello.txt", HgStatusCode::REMOVED}});

  // Calling `hg remove` again should have no effect and not throw any errors.
  scmRemoveFile(dirstate, "hello.txt", /* force */ false);
  EXPECT_TRUE(testMount->hasFileAt("hello.txt"));
  verifyExpectedDirstate(dirstate, {{"hello.txt", HgStatusCode::REMOVED}});
}

TEST(Dirstate, createDirstateHgAddFileRemoveItThenHgRemoveIt) {
  TestMountBuilder builder;
  auto testMount = builder.build();
  auto dirstate = testMount->getDirstate();

  testMount->addFile("hello.txt", "I will be added.");
  dirstate->add(RelativePathPiece("hello.txt"));
  verifyExpectedDirstate(dirstate, {{"hello.txt", HgStatusCode::ADDED}});

  testMount->deleteFile("hello.txt");
  verifyExpectedDirstate(dirstate, {{"hello.txt", HgStatusCode::MISSING}});

  scmRemoveFile(dirstate, "hello.txt", /* force */ false);
  verifyEmptyDirstate(dirstate);
}

TEST(Dirstate, createDirstateHgAddFileRemoveItThenHgRemoveItInSubdirectory) {
  TestMountBuilder builder;
  auto testMount = builder.build();
  auto dirstate = testMount->getDirstate();

  testMount->mkdir("dir1");
  testMount->mkdir("dir1/dir2");
  testMount->addFile("dir1/dir2/hello.txt", "I will be added.");
  dirstate->add(RelativePathPiece("dir1/dir2/hello.txt"));
  verifyExpectedDirstate(
      dirstate, {{"dir1/dir2/hello.txt", HgStatusCode::ADDED}});

  testMount->deleteFile("dir1/dir2/hello.txt");
  testMount->rmdir("dir1/dir2");
  verifyExpectedDirstate(
      dirstate, {{"dir1/dir2/hello.txt", HgStatusCode::MISSING}});

  scmRemoveFile(dirstate, "dir1/dir2/hello.txt", /* force */ false);
  verifyEmptyDirstate(dirstate);
}

TEST(Dirstate, createDirstateHgAddFileThenHgRemoveIt) {
  TestMountBuilder builder;
  auto testMount = builder.build();
  auto dirstate = testMount->getDirstate();

  testMount->addFile("hello.txt", "I will be added.");
  dirstate->add(RelativePathPiece("hello.txt"));
  verifyExpectedDirstate(dirstate, {{"hello.txt", HgStatusCode::ADDED}});

  scmRemoveFileAndExpect(
      dirstate,
      "hello.txt",
      /* force */ false,
      DirstateRemoveError{
          RelativePath("hello.txt"),
          "not removing hello.txt: file has been marked for add "
          "(use 'hg forget' to undo add)"});
  verifyExpectedDirstate(dirstate, {{"hello.txt", HgStatusCode::ADDED}});
}

TEST(Dirstate, createDirstateWithFileAndThenDeleteItWithoutCallingHgRemove) {
  TestMountBuilder builder;
  builder.addFile({"hello.txt", "some contents"});
  auto testMount = builder.build();
  auto dirstate = testMount->getDirstate();

  testMount->deleteFile("hello.txt");
  verifyExpectedDirstate(dirstate, {{"hello.txt", HgStatusCode::MISSING}});
}

TEST(Dirstate, removeAllOnADirectoryWithFilesInVariousStates) {
  TestMountBuilder builder;
  builder.addFiles({
      {"mydir/a", "In the manifest."},
      {"mydir/b", "Will rm."},
      {"mydir/c", "Will hg rm."},
  });
  auto testMount = builder.build();
  auto dirstate = testMount->getDirstate();

  testMount->deleteFile("mydir/b");
  scmRemoveFile(dirstate, "mydir/c", /* force */ false);
  testMount->addFile("mydir/d", "I will be added.");
  dirstate->add(RelativePathPiece("mydir/d"));
  testMount->addFile("mydir/e", "I will be untracked");
  verifyExpectedDirstate(
      dirstate,
      {{"mydir/b", HgStatusCode::MISSING},
       {"mydir/c", HgStatusCode::REMOVED},
       {"mydir/d", HgStatusCode::ADDED},
       {"mydir/e", HgStatusCode::NOT_TRACKED}});

  scmRemoveFileAndExpect(
      dirstate,
      "mydir",
      /* force */ false,
      DirstateRemoveError{
          RelativePath("mydir/d"),
          "not removing mydir/d: "
          "file has been marked for add (use 'hg forget' to undo add)"});
  verifyExpectedDirstate(
      dirstate,
      {{"mydir/a", HgStatusCode::REMOVED},
       {"mydir/b", HgStatusCode::REMOVED},
       {"mydir/c", HgStatusCode::REMOVED},
       {"mydir/d", HgStatusCode::ADDED},
       {"mydir/e", HgStatusCode::NOT_TRACKED}});
  EXPECT_FALSE(testMount->hasFileAt("mydir/a"));
  EXPECT_FALSE(testMount->hasFileAt("mydir/b"));
  EXPECT_FALSE(testMount->hasFileAt("mydir/c"));
  EXPECT_TRUE(testMount->hasFileAt("mydir/d"));
  EXPECT_TRUE(testMount->hasFileAt("mydir/e"));
}

TEST(Dirstate, createDirstateAndAddNewDirectory) {
  TestMountBuilder builder;
  builder.addFile({"file-in-root.txt", "some contents"});
  auto testMount = builder.build();
  auto dirstate = testMount->getDirstate();

  // Add one folder that appears before file-in-root.txt alphabetically.
  testMount->mkdir("a-new-folder");
  testMount->addFile("a-new-folder/add.txt", "");
  testMount->addFile("a-new-folder/not-tracked.txt", "");
  dirstate->add(RelativePathPiece("a-new-folder/add.txt"));

  // Add one folder that appears after file-in-root.txt alphabetically.
  testMount->mkdir("z-new-folder");
  testMount->addFile("z-new-folder/add.txt", "");
  testMount->addFile("z-new-folder/not-tracked.txt", "");
  dirstate->add(RelativePathPiece("z-new-folder/add.txt"));

  verifyExpectedDirstate(
      dirstate,
      {
          {"a-new-folder/add.txt", HgStatusCode::ADDED},
          {"a-new-folder/not-tracked.txt", HgStatusCode::NOT_TRACKED},
          {"z-new-folder/add.txt", HgStatusCode::ADDED},
          {"z-new-folder/not-tracked.txt", HgStatusCode::NOT_TRACKED},
      });
}

TEST(Dirstate, createDirstateAndRemoveExistingDirectory) {
  TestMountBuilder builder;
  builder.addFile({"file-in-root.txt", "some contents"});

  // Add one folder that appears before file-in-root.txt alphabetically.
  builder.addFile({"a-new-folder/original1.txt", ""});
  builder.addFile({"a-new-folder/original2.txt", ""});

  // Add one folder that appears after file-in-root.txt alphabetically.
  builder.addFile({"z-new-folder/original1.txt", ""});
  builder.addFile({"z-new-folder/original2.txt", ""});

  auto testMount = builder.build();
  auto dirstate = testMount->getDirstate();

  // Remove some files in the directories.
  auto force = false;
  scmRemoveFile(dirstate, "a-new-folder/original1.txt", force);
  scmRemoveFile(dirstate, "z-new-folder/original1.txt", force);
  verifyExpectedDirstate(
      dirstate,
      {
          {"a-new-folder/original1.txt", HgStatusCode::REMOVED},
          {"z-new-folder/original1.txt", HgStatusCode::REMOVED},
      });

  // Remove the remaining files in the directories.
  scmRemoveFile(dirstate, "a-new-folder/original2.txt", force);
  scmRemoveFile(dirstate, "z-new-folder/original2.txt", force);
  verifyExpectedDirstate(
      dirstate,
      {
          {"a-new-folder/original1.txt", HgStatusCode::REMOVED},
          {"a-new-folder/original2.txt", HgStatusCode::REMOVED},
          {"z-new-folder/original1.txt", HgStatusCode::REMOVED},
          {"z-new-folder/original2.txt", HgStatusCode::REMOVED},
      });

  // Deleting the directories should not change the results.
  testMount->rmdir("a-new-folder");
  testMount->rmdir("z-new-folder");
  verifyExpectedDirstate(
      dirstate,
      {
          {"a-new-folder/original1.txt", HgStatusCode::REMOVED},
          {"a-new-folder/original2.txt", HgStatusCode::REMOVED},
          {"z-new-folder/original1.txt", HgStatusCode::REMOVED},
          {"z-new-folder/original2.txt", HgStatusCode::REMOVED},
      });
}

TEST(Dirstate, createDirstateAndReplaceFileWithDirectory) {
  TestMountBuilder builder;
  builder.addFile({"dir/some-file", ""});

  auto testMount = builder.build();
  auto dirstate = testMount->getDirstate();

  // Replace file with empty directory.
  testMount->deleteFile("dir/some-file");
  testMount->mkdir("dir/some-file");
  verifyExpectedDirstate(
      dirstate,
      {
          {"dir/some-file", HgStatusCode::MISSING},
      });

  // Add file to new, empty directory.
  testMount->addFile("dir/some-file/a-real-file.txt", "");
  verifyExpectedDirstate(
      dirstate,
      {
          {"dir/some-file", HgStatusCode::MISSING},
          {"dir/some-file/a-real-file.txt", HgStatusCode::NOT_TRACKED},
      });

  // TODO: Trying to `hg add dir/some-file/a-real-file.txt` should fail with:
  // "abort: file 'dir/some-file' in dirstate clashes with
  //     'dir/some-file/a-real-file.txt'"
  // dirstate->add(RelativePathPiece("dir/some-file/a-real-file.txt"));
}

TEST(Dirstate, createDirstateAndReplaceDirectoryWithFile) {
  TestMountBuilder builder;
  builder.addFile({"dir1/dir2/some-file", ""});

  auto testMount = builder.build();
  auto dirstate = testMount->getDirstate();

  testMount->deleteFile("dir1/dir2/some-file");
  testMount->rmdir("dir1/dir2");
  verifyExpectedDirstate(
      dirstate,
      {
          {"dir1/dir2/some-file", HgStatusCode::MISSING},
      });

  testMount->addFile("dir1/dir2", "");
  verifyExpectedDirstate(
      dirstate,
      {
          {"dir1/dir2", HgStatusCode::NOT_TRACKED},
          {"dir1/dir2/some-file", HgStatusCode::MISSING},
      });

  // TODO: Trying to `hg add dir1/dir2` should fail with:
  // "abort: directory 'dir1/dir2' already in dirstate"
  // dirstate->add(RelativePathPiece("dir1/dir2"));
}

TEST(Dirstate, createDirstateAndAddSubtree) {
  TestMountBuilder builder;

  auto testMount = builder.build();
  auto dirstate = testMount->getDirstate();

  testMount->addFile("root1.txt", "");
  testMount->addFile("root2.txt", "");
  testMount->mkdir("dir1");
  testMount->addFile("dir1/aFile.txt", "");
  testMount->addFile("dir1/bFile.txt", "");
  dirstate->add(RelativePathPiece("root1.txt"));
  dirstate->add(RelativePathPiece("dir1/bFile.txt"));
  verifyExpectedDirstate(
      dirstate,
      {
          {"root1.txt", HgStatusCode::ADDED},
          {"root2.txt", HgStatusCode::NOT_TRACKED},
          {"dir1/aFile.txt", HgStatusCode::NOT_TRACKED},
          {"dir1/bFile.txt", HgStatusCode::ADDED},
      });

  testMount->mkdir("dir1/dir2");
  testMount->mkdir("dir1/dir2/dir3");
  testMount->mkdir("dir1/dir2/dir3/dir4");
  testMount->addFile("dir1/dir2/dir3/dir4/cFile.txt", "");
  verifyExpectedDirstate(
      dirstate,
      {
          {"root1.txt", HgStatusCode::ADDED},
          {"root2.txt", HgStatusCode::NOT_TRACKED},
          {"dir1/aFile.txt", HgStatusCode::NOT_TRACKED},
          {"dir1/bFile.txt", HgStatusCode::ADDED},
          {"dir1/dir2/dir3/dir4/cFile.txt", HgStatusCode::NOT_TRACKED},
      });

  dirstate->add(RelativePathPiece("dir1/dir2/dir3/dir4/cFile.txt"));
  verifyExpectedDirstate(
      dirstate,
      {
          {"root1.txt", HgStatusCode::ADDED},
          {"root2.txt", HgStatusCode::NOT_TRACKED},
          {"dir1/aFile.txt", HgStatusCode::NOT_TRACKED},
          {"dir1/bFile.txt", HgStatusCode::ADDED},
          {"dir1/dir2/dir3/dir4/cFile.txt", HgStatusCode::ADDED},
      });
}

TEST(Dirstate, createDirstateAndRemoveSubtree) {
  TestMountBuilder builder;
  builder.addFile({"root.txt", ""});
  builder.addFile({"dir1/a-file.txt", ""});
  builder.addFile({"dir1/b-file.txt", ""});
  builder.addFile({"dir1/dir2/a-file.txt", ""});
  builder.addFile({"dir1/dir2/b-file.txt", ""});
  builder.addFile({"dir1/dir2/dir3/dir4/a-file.txt", ""});
  builder.addFile({"dir1/dir2/dir3/dir4/b-file.txt", ""});

  auto testMount = builder.build();
  auto dirstate = testMount->getDirstate();

  testMount->deleteFile("dir1/dir2/dir3/dir4/a-file.txt");
  verifyExpectedDirstate(
      dirstate,
      {
          {"dir1/dir2/dir3/dir4/a-file.txt", HgStatusCode::MISSING},
      });

  testMount->deleteFile("dir1/dir2/dir3/dir4/b-file.txt");
  verifyExpectedDirstate(
      dirstate,
      {
          {"dir1/dir2/dir3/dir4/a-file.txt", HgStatusCode::MISSING},
          {"dir1/dir2/dir3/dir4/b-file.txt", HgStatusCode::MISSING},
      });

  testMount->rmdir("dir1/dir2/dir3/dir4");
  verifyExpectedDirstate(
      dirstate,
      {
          {"dir1/dir2/dir3/dir4/a-file.txt", HgStatusCode::MISSING},
          {"dir1/dir2/dir3/dir4/b-file.txt", HgStatusCode::MISSING},
      });

  testMount->rmdir("dir1/dir2/dir3");
  verifyExpectedDirstate(
      dirstate,
      {
          {"dir1/dir2/dir3/dir4/a-file.txt", HgStatusCode::MISSING},
          {"dir1/dir2/dir3/dir4/b-file.txt", HgStatusCode::MISSING},
      });

  testMount->deleteFile("dir1/dir2/a-file.txt");
  testMount->deleteFile("dir1/dir2/b-file.txt");
  verifyExpectedDirstate(
      dirstate,
      {
          {"dir1/dir2/a-file.txt", HgStatusCode::MISSING},
          {"dir1/dir2/b-file.txt", HgStatusCode::MISSING},
          {"dir1/dir2/dir3/dir4/a-file.txt", HgStatusCode::MISSING},
          {"dir1/dir2/dir3/dir4/b-file.txt", HgStatusCode::MISSING},
      });

  testMount->deleteFile("dir1/a-file.txt");
  testMount->deleteFile("dir1/b-file.txt");
  verifyExpectedDirstate(
      dirstate,
      {
          {"dir1/a-file.txt", HgStatusCode::MISSING},
          {"dir1/b-file.txt", HgStatusCode::MISSING},
          {"dir1/dir2/a-file.txt", HgStatusCode::MISSING},
          {"dir1/dir2/b-file.txt", HgStatusCode::MISSING},
          {"dir1/dir2/dir3/dir4/a-file.txt", HgStatusCode::MISSING},
          {"dir1/dir2/dir3/dir4/b-file.txt", HgStatusCode::MISSING},
      });

  testMount->deleteFile("root.txt");
  testMount->rmdir("dir1/dir2");
  testMount->rmdir("dir1");
  verifyExpectedDirstate(
      dirstate,
      {
          {"root.txt", HgStatusCode::MISSING},
          {"dir1/a-file.txt", HgStatusCode::MISSING},
          {"dir1/b-file.txt", HgStatusCode::MISSING},
          {"dir1/dir2/a-file.txt", HgStatusCode::MISSING},
          {"dir1/dir2/b-file.txt", HgStatusCode::MISSING},
          {"dir1/dir2/dir3/dir4/a-file.txt", HgStatusCode::MISSING},
          {"dir1/dir2/dir3/dir4/b-file.txt", HgStatusCode::MISSING},
      });
}

TEST(Dirstate, checkIgnoredBehavior) {
  TestMountBuilder builder;
  builder.addFiles({
      {".gitignore", "hello*\n"},
      {"a/b/c/noop.c", "int main() { return 0; }\n"},
  });
  auto testMount = builder.build();
  testMount->addFile("hello.txt", "some contents");
  testMount->addFile("goodbye.txt", "other contents");
  testMount->addFile(
      "a/b/c/noop.o",
      "\x7f"
      "ELF");

  auto dirstate = testMount->getDirstate();

  verifyExpectedDirstate(
      dirstate,
      {
          {"hello.txt", HgStatusCode::IGNORED},
          {"goodbye.txt", HgStatusCode::NOT_TRACKED},
          {"a/b/c/noop.o", HgStatusCode::NOT_TRACKED},
      });

  testMount->addFile("a/b/.gitignore", "*.o\n");
  verifyExpectedDirstate(
      dirstate,
      {
          {"hello.txt", HgStatusCode::IGNORED},
          {"goodbye.txt", HgStatusCode::NOT_TRACKED},
          {"a/b/.gitignore", HgStatusCode::NOT_TRACKED},
          {"a/b/c/noop.o", HgStatusCode::IGNORED},
      });
}
