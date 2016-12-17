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
  std::unordered_map<RelativePath, StatusCode> statuses({{
      {RelativePath("clean.txt"), StatusCode::CLEAN},
      {RelativePath("modified.txt"), StatusCode::MODIFIED},
      {RelativePath("added.txt"), StatusCode::ADDED},
      {RelativePath("removed.txt"), StatusCode::REMOVED},
      {RelativePath("missing.txt"), StatusCode::MISSING},
      {RelativePath("not_tracked.txt"), StatusCode::NOT_TRACKED},
      {RelativePath("ignored.txt"), StatusCode::IGNORED},
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
    std::unordered_map<std::string, StatusCode>&& statuses) {
  std::unordered_map<RelativePath, StatusCode> expected;
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
 * Calls `dirstate->addAll({path}, errorsToReport)` and fails if
 * errorsToReport is non-empty. Note that path may identify a file or a
 * directory, though it must be an existing file.
 */
void scmAddFile(Dirstate* dirstate, std::string path) {
  std::vector<DirstateAddRemoveError> errorsToReport;
  std::vector<RelativePathPiece> paths({RelativePathPiece(path)});
  dirstate->addAll(paths, &errorsToReport);
  if (!errorsToReport.empty()) {
    FAIL() << "Unexpected error: " << errorsToReport[0];
  }
}

void scmAddFileAndExpect(
    Dirstate* dirstate,
    std::string path,
    DirstateAddRemoveError expectedError) {
  std::vector<DirstateAddRemoveError> errorsToReport;
  std::vector<RelativePathPiece> paths({RelativePathPiece(path)});
  dirstate->addAll(paths, &errorsToReport);
  std::vector<DirstateAddRemoveError> expectedErrors({expectedError});
  EXPECT_EQ(expectedErrors, errorsToReport);
}

/**
 * Calls `dirstate->removeAll({path}, force, errorsToReport)` and fails if
 * errorsToReport is non-empty.
 */
void scmRemoveFile(Dirstate* dirstate, std::string path, bool force) {
  std::vector<DirstateAddRemoveError> errorsToReport;
  std::vector<RelativePathPiece> paths({RelativePathPiece(path)});
  dirstate->removeAll(paths, force, &errorsToReport);
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
    DirstateAddRemoveError expectedError) {
  std::vector<DirstateAddRemoveError> errorsToReport;
  std::vector<RelativePathPiece> paths({RelativePathPiece(path)});
  dirstate->removeAll(paths, force, &errorsToReport);
  std::vector<DirstateAddRemoveError> expectedErrors({expectedError});
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
          {"deleted.txt", StatusCode::REMOVED},
          {"missing.txt", StatusCode::MISSING},
          {"newfile.txt", StatusCode::ADDED},
          {"removed.txt", StatusCode::REMOVED},
      });
}

TEST(Dirstate, createDirstateWithUntrackedFile) {
  TestMountBuilder builder;
  auto testMount = builder.build();
  auto dirstate = testMount->getDirstate();

  testMount->addFile("hello.txt", "some contents");
  verifyExpectedDirstate(dirstate, {{"hello.txt", StatusCode::NOT_TRACKED}});
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
  scmAddFile(dirstate, "hello.txt");
  verifyExpectedDirstate(dirstate, {{"hello.txt", StatusCode::ADDED}});
}

TEST(Dirstate, createDirstateWithMissingFile) {
  TestMountBuilder builder;
  auto testMount = builder.build();
  auto dirstate = testMount->getDirstate();

  testMount->addFile("hello.txt", "some contents");
  scmAddFile(dirstate, "hello.txt");
  testMount->deleteFile("hello.txt");
  verifyExpectedDirstate(dirstate, {{"hello.txt", StatusCode::MISSING}});
}

TEST(Dirstate, createDirstateWithModifiedFileContents) {
  TestMountBuilder builder;
  builder.addFile({"hello.txt", "some contents"});
  auto testMount = builder.build();
  auto dirstate = testMount->getDirstate();

  testMount->overwriteFile("hello.txt", "other contents");
  verifyExpectedDirstate(dirstate, {{"hello.txt", StatusCode::MODIFIED}});
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

TEST(Dirstate, addDirectoriesWithMixOfFiles) {
  TestMountBuilder builder;
  builder.addFiles({
      {"rootfile.txt", ""}, {"dir1/a.txt", "original contents"},
  });
  auto testMount = builder.build();

  testMount->addFile("dir1/b.txt", "");
  testMount->mkdir("dir2");
  testMount->addFile("dir2/c.txt", "");

  auto dirstate = testMount->getDirstate();
  verifyExpectedDirstate(
      dirstate,
      {
          {"dir1/b.txt", StatusCode::NOT_TRACKED},
          {"dir2/c.txt", StatusCode::NOT_TRACKED},
      });

  // `hg add dir2` should ensure only things under dir2 are added.
  scmAddFile(dirstate, "dir2");
  verifyExpectedDirstate(
      dirstate,
      {
          {"dir1/b.txt", StatusCode::NOT_TRACKED},
          {"dir2/c.txt", StatusCode::ADDED},
      });

  // This is the equivalent of `hg forget dir1/a.txt`.
  scmRemoveFile(dirstate, "dir1/a.txt", /* force */ false);
  testMount->addFile("dir1/a.txt", "original contents");
  verifyExpectedDirstate(
      dirstate,
      {
          {"dir1/a.txt", StatusCode::REMOVED},
          {"dir1/b.txt", StatusCode::NOT_TRACKED},
          {"dir2/c.txt", StatusCode::ADDED},
      });

  // Running `hg add .` should remove the removal marker from dir1/a.txt because
  // dir1/a.txt is still on disk.
  scmAddFile(dirstate, "");
  verifyExpectedDirstate(
      dirstate,
      {
          {"dir1/b.txt", StatusCode::ADDED}, {"dir2/c.txt", StatusCode::ADDED},
      });

  scmRemoveFile(dirstate, "dir1/a.txt", /* force */ false);
  testMount->addFile("dir1/a.txt", "different contents");
  // Running `hg add dir1` should remove the removal marker from dir1/a.txt, but
  // `hg status` should also reflect that it is modified.
  scmAddFile(dirstate, "dir1");
  verifyExpectedDirstate(
      dirstate,
      {
          {"dir1/a.txt", StatusCode::MODIFIED},
          {"dir1/b.txt", StatusCode::ADDED},
          {"dir2/c.txt", StatusCode::ADDED},
      });

  scmRemoveFile(dirstate, "dir1/a.txt", /* force */ true);
  // This should not add dir1/a.txt back because it is not on disk.
  scmAddFile(dirstate, "dir1");
  verifyExpectedDirstate(
      dirstate,
      {
          {"dir1/a.txt", StatusCode::REMOVED},
          {"dir1/b.txt", StatusCode::ADDED},
          {"dir2/c.txt", StatusCode::ADDED},
      });

  scmAddFileAndExpect(
      dirstate,
      "dir3",
      DirstateAddRemoveError{RelativePath("dir3"),
                             "dir3: No such file or directory"}

      );
}

TEST(Dirstate, createDirstateWithFileAndThenHgRemoveIt) {
  TestMountBuilder builder;
  builder.addFile({"hello.txt", "some contents"});
  auto testMount = builder.build();
  auto dirstate = testMount->getDirstate();

  scmRemoveFile(dirstate, "hello.txt", /* force */ false);
  EXPECT_FALSE(testMount->hasFileAt("hello.txt"));

  verifyExpectedDirstate(dirstate, {{"hello.txt", StatusCode::REMOVED}});
}

TEST(Dirstate, createDirstateWithFileRemoveItAndThenHgRemoveIt) {
  TestMountBuilder builder;
  builder.addFile({"hello.txt", "some contents"});
  auto testMount = builder.build();
  auto dirstate = testMount->getDirstate();

  testMount->deleteFile("hello.txt");
  scmRemoveFile(dirstate, "hello.txt", /* force */ false);

  verifyExpectedDirstate(dirstate, {{"hello.txt", StatusCode::REMOVED}});
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
      DirstateAddRemoveError{RelativePath("hello.txt"),
                             "not removing hello.txt: file is modified "
                             "(use -f to force removal)"});

  testMount->overwriteFile("hello.txt", "original contents");
  scmRemoveFile(dirstate, "hello.txt", /* force */ false);
  EXPECT_FALSE(testMount->hasFileAt("hello.txt"));

  verifyExpectedDirstate(dirstate, {{"hello.txt", StatusCode::REMOVED}});
}

TEST(Dirstate, createDirstateWithFileModifyItAndThenHgForceRemoveIt) {
  TestMountBuilder builder;
  builder.addFile({"hello.txt", "original contents"});
  auto testMount = builder.build();
  auto dirstate = testMount->getDirstate();

  testMount->overwriteFile("hello.txt", "some other contents");
  scmRemoveFile(dirstate, "hello.txt", /* force */ true);
  EXPECT_FALSE(testMount->hasFileAt("hello.txt"));
  verifyExpectedDirstate(dirstate, {{"hello.txt", StatusCode::REMOVED}});
}

TEST(Dirstate, ensureSubsequentCallsToHgRemoveHaveNoEffect) {
  TestMountBuilder builder;
  builder.addFile({"hello.txt", "original contents"});
  auto testMount = builder.build();
  auto dirstate = testMount->getDirstate();

  scmRemoveFile(dirstate, "hello.txt", /* force */ false);
  EXPECT_FALSE(testMount->hasFileAt("hello.txt"));
  verifyExpectedDirstate(dirstate, {{"hello.txt", StatusCode::REMOVED}});

  // Calling `hg remove` again should have no effect and not throw any errors.
  scmRemoveFile(dirstate, "hello.txt", /* force */ false);
  EXPECT_FALSE(testMount->hasFileAt("hello.txt"));
  verifyExpectedDirstate(dirstate, {{"hello.txt", StatusCode::REMOVED}});

  // Even if we restore the file, it should still show up as removed in
  // `hg status`.
  testMount->addFile("hello.txt", "original contents");
  EXPECT_TRUE(testMount->hasFileAt("hello.txt"));
  verifyExpectedDirstate(dirstate, {{"hello.txt", StatusCode::REMOVED}});

  // Calling `hg remove` again should have no effect and not throw any errors.
  scmRemoveFile(dirstate, "hello.txt", /* force */ false);
  EXPECT_TRUE(testMount->hasFileAt("hello.txt"));
  verifyExpectedDirstate(dirstate, {{"hello.txt", StatusCode::REMOVED}});
}

TEST(Dirstate, createDirstateHgAddFileRemoveItThenHgRemoveIt) {
  TestMountBuilder builder;
  auto testMount = builder.build();
  auto dirstate = testMount->getDirstate();

  testMount->addFile("hello.txt", "I will be added.");
  scmAddFile(dirstate, "hello.txt");
  verifyExpectedDirstate(dirstate, {{"hello.txt", StatusCode::ADDED}});

  testMount->deleteFile("hello.txt");
  verifyExpectedDirstate(dirstate, {{"hello.txt", StatusCode::MISSING}});

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
  scmAddFile(dirstate, "dir1/dir2/hello.txt");
  verifyExpectedDirstate(
      dirstate, {{"dir1/dir2/hello.txt", StatusCode::ADDED}});

  testMount->deleteFile("dir1/dir2/hello.txt");
  testMount->rmdir("dir1/dir2");
  verifyExpectedDirstate(
      dirstate, {{"dir1/dir2/hello.txt", StatusCode::MISSING}});

  scmRemoveFile(dirstate, "dir1/dir2/hello.txt", /* force */ false);
  verifyEmptyDirstate(dirstate);
}

TEST(Dirstate, createDirstateHgAddFileThenHgRemoveIt) {
  TestMountBuilder builder;
  auto testMount = builder.build();
  auto dirstate = testMount->getDirstate();

  testMount->addFile("hello.txt", "I will be added.");
  scmAddFile(dirstate, "hello.txt");
  verifyExpectedDirstate(dirstate, {{"hello.txt", StatusCode::ADDED}});

  scmRemoveFileAndExpect(
      dirstate,
      "hello.txt",
      /* force */ false,
      DirstateAddRemoveError{
          RelativePath("hello.txt"),
          "not removing hello.txt: file has been marked for add "
          "(use 'hg forget' to undo add)"});
  verifyExpectedDirstate(dirstate, {{"hello.txt", StatusCode::ADDED}});
}

TEST(Dirstate, createDirstateWithFileAndThenDeleteItWithoutCallingHgRemove) {
  TestMountBuilder builder;
  builder.addFile({"hello.txt", "some contents"});
  auto testMount = builder.build();
  auto dirstate = testMount->getDirstate();

  testMount->deleteFile("hello.txt");
  verifyExpectedDirstate(dirstate, {{"hello.txt", StatusCode::MISSING}});
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
  scmAddFile(dirstate, "mydir/d");
  testMount->addFile("mydir/e", "I will be untracked");
  verifyExpectedDirstate(
      dirstate,
      {{"mydir/b", StatusCode::MISSING},
       {"mydir/c", StatusCode::REMOVED},
       {"mydir/d", StatusCode::ADDED},
       {"mydir/e", StatusCode::NOT_TRACKED}});

  scmRemoveFileAndExpect(
      dirstate,
      "mydir",
      /* force */ false,
      DirstateAddRemoveError{
          RelativePath("mydir/d"),
          "not removing mydir/d: "
          "file has been marked for add (use 'hg forget' to undo add)"});
  verifyExpectedDirstate(
      dirstate,
      {{"mydir/a", StatusCode::REMOVED},
       {"mydir/b", StatusCode::REMOVED},
       {"mydir/c", StatusCode::REMOVED},
       {"mydir/d", StatusCode::ADDED},
       {"mydir/e", StatusCode::NOT_TRACKED}});
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
  scmAddFile(dirstate, "a-new-folder/add.txt");

  // Add one folder that appears after file-in-root.txt alphabetically.
  testMount->mkdir("z-new-folder");
  testMount->addFile("z-new-folder/add.txt", "");
  testMount->addFile("z-new-folder/not-tracked.txt", "");
  scmAddFile(dirstate, "z-new-folder/add.txt");

  verifyExpectedDirstate(
      dirstate,
      {
          {"a-new-folder/add.txt", StatusCode::ADDED},
          {"a-new-folder/not-tracked.txt", StatusCode::NOT_TRACKED},
          {"z-new-folder/add.txt", StatusCode::ADDED},
          {"z-new-folder/not-tracked.txt", StatusCode::NOT_TRACKED},
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
          {"a-new-folder/original1.txt", StatusCode::REMOVED},
          {"z-new-folder/original1.txt", StatusCode::REMOVED},
      });

  // Remove the remaining files in the directories.
  scmRemoveFile(dirstate, "a-new-folder/original2.txt", force);
  scmRemoveFile(dirstate, "z-new-folder/original2.txt", force);
  verifyExpectedDirstate(
      dirstate,
      {
          {"a-new-folder/original1.txt", StatusCode::REMOVED},
          {"a-new-folder/original2.txt", StatusCode::REMOVED},
          {"z-new-folder/original1.txt", StatusCode::REMOVED},
          {"z-new-folder/original2.txt", StatusCode::REMOVED},
      });

  // Deleting the directories should not change the results.
  testMount->rmdir("a-new-folder");
  testMount->rmdir("z-new-folder");
  verifyExpectedDirstate(
      dirstate,
      {
          {"a-new-folder/original1.txt", StatusCode::REMOVED},
          {"a-new-folder/original2.txt", StatusCode::REMOVED},
          {"z-new-folder/original1.txt", StatusCode::REMOVED},
          {"z-new-folder/original2.txt", StatusCode::REMOVED},
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
          {"dir/some-file", StatusCode::MISSING},
      });

  // Add file to new, empty directory.
  testMount->addFile("dir/some-file/a-real-file.txt", "");
  verifyExpectedDirstate(
      dirstate,
      {
          {"dir/some-file", StatusCode::MISSING},
          {"dir/some-file/a-real-file.txt", StatusCode::NOT_TRACKED},
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
          {"dir1/dir2/some-file", StatusCode::MISSING},
      });

  testMount->addFile("dir1/dir2", "");
  verifyExpectedDirstate(
      dirstate,
      {
          {"dir1/dir2", StatusCode::NOT_TRACKED},
          {"dir1/dir2/some-file", StatusCode::MISSING},
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
  scmAddFile(dirstate, "root1.txt");
  scmAddFile(dirstate, "dir1/bFile.txt");
  verifyExpectedDirstate(
      dirstate,
      {
          {"root1.txt", StatusCode::ADDED},
          {"root2.txt", StatusCode::NOT_TRACKED},
          {"dir1/aFile.txt", StatusCode::NOT_TRACKED},
          {"dir1/bFile.txt", StatusCode::ADDED},
      });

  testMount->mkdir("dir1/dir2");
  testMount->mkdir("dir1/dir2/dir3");
  testMount->mkdir("dir1/dir2/dir3/dir4");
  testMount->addFile("dir1/dir2/dir3/dir4/cFile.txt", "");
  verifyExpectedDirstate(
      dirstate,
      {
          {"root1.txt", StatusCode::ADDED},
          {"root2.txt", StatusCode::NOT_TRACKED},
          {"dir1/aFile.txt", StatusCode::NOT_TRACKED},
          {"dir1/bFile.txt", StatusCode::ADDED},
          {"dir1/dir2/dir3/dir4/cFile.txt", StatusCode::NOT_TRACKED},
      });

  scmAddFile(dirstate, "dir1/dir2/dir3/dir4/cFile.txt");
  verifyExpectedDirstate(
      dirstate,
      {
          {"root1.txt", StatusCode::ADDED},
          {"root2.txt", StatusCode::NOT_TRACKED},
          {"dir1/aFile.txt", StatusCode::NOT_TRACKED},
          {"dir1/bFile.txt", StatusCode::ADDED},
          {"dir1/dir2/dir3/dir4/cFile.txt", StatusCode::ADDED},
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
          {"dir1/dir2/dir3/dir4/a-file.txt", StatusCode::MISSING},
      });

  testMount->deleteFile("dir1/dir2/dir3/dir4/b-file.txt");
  verifyExpectedDirstate(
      dirstate,
      {
          {"dir1/dir2/dir3/dir4/a-file.txt", StatusCode::MISSING},
          {"dir1/dir2/dir3/dir4/b-file.txt", StatusCode::MISSING},
      });

  testMount->rmdir("dir1/dir2/dir3/dir4");
  verifyExpectedDirstate(
      dirstate,
      {
          {"dir1/dir2/dir3/dir4/a-file.txt", StatusCode::MISSING},
          {"dir1/dir2/dir3/dir4/b-file.txt", StatusCode::MISSING},
      });

  testMount->rmdir("dir1/dir2/dir3");
  verifyExpectedDirstate(
      dirstate,
      {
          {"dir1/dir2/dir3/dir4/a-file.txt", StatusCode::MISSING},
          {"dir1/dir2/dir3/dir4/b-file.txt", StatusCode::MISSING},
      });

  testMount->deleteFile("dir1/dir2/a-file.txt");
  testMount->deleteFile("dir1/dir2/b-file.txt");
  verifyExpectedDirstate(
      dirstate,
      {
          {"dir1/dir2/a-file.txt", StatusCode::MISSING},
          {"dir1/dir2/b-file.txt", StatusCode::MISSING},
          {"dir1/dir2/dir3/dir4/a-file.txt", StatusCode::MISSING},
          {"dir1/dir2/dir3/dir4/b-file.txt", StatusCode::MISSING},
      });

  testMount->deleteFile("dir1/a-file.txt");
  testMount->deleteFile("dir1/b-file.txt");
  verifyExpectedDirstate(
      dirstate,
      {
          {"dir1/a-file.txt", StatusCode::MISSING},
          {"dir1/b-file.txt", StatusCode::MISSING},
          {"dir1/dir2/a-file.txt", StatusCode::MISSING},
          {"dir1/dir2/b-file.txt", StatusCode::MISSING},
          {"dir1/dir2/dir3/dir4/a-file.txt", StatusCode::MISSING},
          {"dir1/dir2/dir3/dir4/b-file.txt", StatusCode::MISSING},
      });

  testMount->deleteFile("root.txt");
  testMount->rmdir("dir1/dir2");
  testMount->rmdir("dir1");
  verifyExpectedDirstate(
      dirstate,
      {
          {"root.txt", StatusCode::MISSING},
          {"dir1/a-file.txt", StatusCode::MISSING},
          {"dir1/b-file.txt", StatusCode::MISSING},
          {"dir1/dir2/a-file.txt", StatusCode::MISSING},
          {"dir1/dir2/b-file.txt", StatusCode::MISSING},
          {"dir1/dir2/dir3/dir4/a-file.txt", StatusCode::MISSING},
          {"dir1/dir2/dir3/dir4/b-file.txt", StatusCode::MISSING},
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
          {"hello.txt", StatusCode::IGNORED},
          {"goodbye.txt", StatusCode::NOT_TRACKED},
          {"a/b/c/noop.o", StatusCode::NOT_TRACKED},
      });

  testMount->addFile("a/b/.gitignore", "*.o\n");
  verifyExpectedDirstate(
      dirstate,
      {
          {"hello.txt", StatusCode::IGNORED},
          {"goodbye.txt", StatusCode::NOT_TRACKED},
          {"a/b/.gitignore", StatusCode::NOT_TRACKED},
          {"a/b/c/noop.o", StatusCode::IGNORED},
      });
}
