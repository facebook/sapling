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

class FakeDirstatePeristence : public DirstatePersistence {
 public:
  virtual ~FakeDirstatePeristence() {}
  void save(std::unordered_map<RelativePath, HgStatusCode>&) override {}
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
