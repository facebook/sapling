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
#include "eden/fs/model/hg/Dirstate.h"
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

class FakeDirstatePeristence : public DirstatePersistence {
 public:
  virtual ~FakeDirstatePeristence() {}
  void save(std::unordered_map<RelativePath, HgUserStatusDirective>&) override {
  }
};

void verifyExpectedDirstate(
    Dirstate& dirstate,
    std::unordered_map<std::string, HgStatusCode>&& statuses) {
  std::unordered_map<RelativePath, HgStatusCode> expected;
  for (auto& pair : statuses) {
    expected.emplace(RelativePath(pair.first), pair.second);
  }
  auto expectedStatus = HgStatus(std::move(expected));
  EXPECT_EQ(expectedStatus, *dirstate.getStatus());
}

void verifyEmptyDirstate(Dirstate& dirstate) {
  auto status = dirstate.getStatus();
  EXPECT_EQ(0, status->size()) << "Expected dirstate to be empty.";
}

TEST(Dirstate, createDirstate) {
  TestMountBuilder builder;
  auto testMount = builder.build();

  auto persistence = std::make_unique<FakeDirstatePeristence>();
  Dirstate dirstate(testMount->getEdenMount(), std::move(persistence));
  verifyEmptyDirstate(dirstate);
}

TEST(Dirstate, createDirstateWithInitialState) {
  TestMountBuilder builder;
  builder.addFile({"removed.txt", "nada"});
  auto testMount = builder.build();
  testMount->addFile("newfile.txt", "legitimate add");

  auto persistence = std::make_unique<FakeDirstatePeristence>();
  std::unordered_map<RelativePath, HgUserStatusDirective> userDirectives{
      {RelativePath("deleted.txt"), HgUserStatusDirective::REMOVE},
      {RelativePath("missing.txt"), HgUserStatusDirective::ADD},
      {RelativePath("newfile.txt"), HgUserStatusDirective::ADD},
      {RelativePath("removed.txt"), HgUserStatusDirective::REMOVE},
  };
  Dirstate dirstate(
      testMount->getEdenMount(), std::move(persistence), &userDirectives);
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
  testMount->addFile("hello.txt", "some contents");

  auto persistence = std::make_unique<FakeDirstatePeristence>();
  Dirstate dirstate(testMount->getEdenMount(), std::move(persistence));

  verifyExpectedDirstate(dirstate, {{"hello.txt", HgStatusCode::NOT_TRACKED}});
}

TEST(Dirstate, createDirstateWithAddedFile) {
  TestMountBuilder builder;
  auto testMount = builder.build();
  testMount->addFile("hello.txt", "some contents");

  auto persistence = std::make_unique<FakeDirstatePeristence>();
  Dirstate dirstate(testMount->getEdenMount(), std::move(persistence));
  dirstate.add(RelativePathPiece("hello.txt"));

  verifyExpectedDirstate(dirstate, {{"hello.txt", HgStatusCode::ADDED}});
}

TEST(Dirstate, createDirstateWithMissingFile) {
  TestMountBuilder builder;
  auto testMount = builder.build();
  testMount->addFile("hello.txt", "some contents");

  auto persistence = std::make_unique<FakeDirstatePeristence>();
  Dirstate dirstate(testMount->getEdenMount(), std::move(persistence));
  dirstate.add(RelativePathPiece("hello.txt"));
  testMount->deleteFile("hello.txt");

  verifyExpectedDirstate(dirstate, {{"hello.txt", HgStatusCode::MISSING}});
}

TEST(Dirstate, createDirstateWithModifiedFileContents) {
  TestMountBuilder builder;
  builder.addFile({"hello.txt", "some contents"});
  auto testMount = builder.build();

  auto persistence = std::make_unique<FakeDirstatePeristence>();
  Dirstate dirstate(testMount->getEdenMount(), std::move(persistence));
  testMount->overwriteFile("hello.txt", "other contents");

  verifyExpectedDirstate(dirstate, {{"hello.txt", HgStatusCode::MODIFIED}});
}

TEST(Dirstate, createDirstateWithTouchedFile) {
  TestMountBuilder builder;
  builder.addFile({"hello.txt", "some contents"});
  auto testMount = builder.build();

  auto persistence = std::make_unique<FakeDirstatePeristence>();
  Dirstate dirstate(testMount->getEdenMount(), std::move(persistence));
  testMount->overwriteFile("hello.txt", "some contents");

  // Although the file has been written, it has not changed in any significant
  // way.
  verifyEmptyDirstate(dirstate);
}

TEST(Dirstate, createDirstateWithFileAndThenHgRemoveIt) {
  TestMountBuilder builder;
  builder.addFile({"hello.txt", "some contents"});
  auto testMount = builder.build();

  auto persistence = std::make_unique<FakeDirstatePeristence>();
  Dirstate dirstate(testMount->getEdenMount(), std::move(persistence));
  dirstate.remove(RelativePathPiece("hello.txt"), /* force */ false);
  EXPECT_FALSE(testMount->hasFileAt("hello.txt"));

  verifyExpectedDirstate(dirstate, {{"hello.txt", HgStatusCode::REMOVED}});
}

TEST(Dirstate, createDirstateWithFileRemoveItAndThenHgRemoveIt) {
  TestMountBuilder builder;
  builder.addFile({"hello.txt", "some contents"});
  auto testMount = builder.build();

  auto persistence = std::make_unique<FakeDirstatePeristence>();
  Dirstate dirstate(testMount->getEdenMount(), std::move(persistence));
  testMount->deleteFile("hello.txt");
  dirstate.remove(RelativePathPiece("hello.txt"), /* force */ false);

  verifyExpectedDirstate(dirstate, {{"hello.txt", HgStatusCode::REMOVED}});
}

TEST(Dirstate, createDirstateWithFileTouchItAndThenHgRemoveIt) {
  TestMountBuilder builder;
  builder.addFile({"hello.txt", "original contents"});
  auto testMount = builder.build();

  auto persistence = std::make_unique<FakeDirstatePeristence>();
  Dirstate dirstate(testMount->getEdenMount(), std::move(persistence));
  testMount->overwriteFile("hello.txt", "some other contents");

  try {
    dirstate.remove(RelativePathPiece("hello.txt"), /* force */ false);
    FAIL() << "Should error when trying to remove a modified file.";
  } catch (const std::runtime_error& e) {
    EXPECT_STREQ(
        "not removing hello.txt: file is modified (use -f to force removal)",
        e.what());
  }

  testMount->overwriteFile("hello.txt", "original contents");
  dirstate.remove(RelativePathPiece("hello.txt"), /* force */ false);
  EXPECT_FALSE(testMount->hasFileAt("hello.txt"));

  verifyExpectedDirstate(dirstate, {{"hello.txt", HgStatusCode::REMOVED}});
}

TEST(Dirstate, createDirstateWithFileModifyItAndThenHgForceRemoveIt) {
  TestMountBuilder builder;
  builder.addFile({"hello.txt", "original contents"});
  auto testMount = builder.build();

  auto persistence = std::make_unique<FakeDirstatePeristence>();
  Dirstate dirstate(testMount->getEdenMount(), std::move(persistence));
  testMount->overwriteFile("hello.txt", "some other contents");

  dirstate.remove(RelativePathPiece("hello.txt"), /* force */ true);
  EXPECT_FALSE(testMount->hasFileAt("hello.txt"));

  verifyExpectedDirstate(dirstate, {{"hello.txt", HgStatusCode::REMOVED}});
}

TEST(Dirstate, ensureSubsequentCallsToHgRemoveHaveNoEffect) {
  TestMountBuilder builder;
  builder.addFile({"hello.txt", "original contents"});
  auto testMount = builder.build();

  auto persistence = std::make_unique<FakeDirstatePeristence>();
  Dirstate dirstate(testMount->getEdenMount(), std::move(persistence));

  dirstate.remove(RelativePathPiece("hello.txt"), /* force */ false);
  EXPECT_FALSE(testMount->hasFileAt("hello.txt"));
  verifyExpectedDirstate(dirstate, {{"hello.txt", HgStatusCode::REMOVED}});

  // Calling `hg remove` again should have no effect and not throw any errors.
  dirstate.remove(RelativePathPiece("hello.txt"), /* force */ false);
  EXPECT_FALSE(testMount->hasFileAt("hello.txt"));
  verifyExpectedDirstate(dirstate, {{"hello.txt", HgStatusCode::REMOVED}});

  // Even if we restore the file, it should still show up as removed in
  // `hg status`.
  testMount->addFile("hello.txt", "original contents");
  EXPECT_TRUE(testMount->hasFileAt("hello.txt"));
  verifyExpectedDirstate(dirstate, {{"hello.txt", HgStatusCode::REMOVED}});

  // Calling `hg remove` again should have no effect and not throw any errors.
  dirstate.remove(RelativePathPiece("hello.txt"), /* force */ false);
  EXPECT_TRUE(testMount->hasFileAt("hello.txt"));
  verifyExpectedDirstate(dirstate, {{"hello.txt", HgStatusCode::REMOVED}});
}

TEST(Dirstate, createDirstateHgAddFileRemoveItThenHgRemoveIt) {
  TestMountBuilder builder;
  auto testMount = builder.build();

  auto persistence = std::make_unique<FakeDirstatePeristence>();
  Dirstate dirstate(testMount->getEdenMount(), std::move(persistence));

  testMount->addFile("hello.txt", "I will be added.");
  dirstate.add(RelativePathPiece("hello.txt"));
  verifyExpectedDirstate(dirstate, {{"hello.txt", HgStatusCode::ADDED}});

  testMount->deleteFile("hello.txt");
  verifyExpectedDirstate(dirstate, {{"hello.txt", HgStatusCode::MISSING}});

  dirstate.remove(RelativePathPiece("hello.txt"), /* force */ false);
  verifyEmptyDirstate(dirstate);
}

TEST(Dirstate, createDirstateHgAddFileThenHgRemoveIt) {
  TestMountBuilder builder;
  auto testMount = builder.build();

  auto persistence = std::make_unique<FakeDirstatePeristence>();
  Dirstate dirstate(testMount->getEdenMount(), std::move(persistence));

  testMount->addFile("hello.txt", "I will be added.");
  dirstate.add(RelativePathPiece("hello.txt"));
  verifyExpectedDirstate(dirstate, {{"hello.txt", HgStatusCode::ADDED}});

  try {
    dirstate.remove(RelativePathPiece("hello.txt"), /* force */ false);
    FAIL() << "Should error when trying to remove a file scheduled for add.";
  } catch (const std::runtime_error& e) {
    EXPECT_STREQ(
        "not removing hello.txt: file has been marked for add "
        "(use 'hg forget' to undo add)",
        e.what());
  }

  verifyExpectedDirstate(dirstate, {{"hello.txt", HgStatusCode::ADDED}});
}

TEST(Dirstate, createDirstateWithFileAndThenDeleteItWithoutCallingHgRemove) {
  TestMountBuilder builder;
  builder.addFile({"hello.txt", "some contents"});
  auto testMount = builder.build();

  auto persistence = std::make_unique<FakeDirstatePeristence>();
  Dirstate dirstate(testMount->getEdenMount(), std::move(persistence));
  testMount->deleteFile("hello.txt");

  verifyExpectedDirstate(dirstate, {{"hello.txt", HgStatusCode::MISSING}});
}
