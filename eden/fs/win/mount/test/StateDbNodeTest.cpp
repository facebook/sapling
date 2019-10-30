/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/win/mount/StateDbNode.h"
#include <eden/fs/model/Hash.h>
#include <eden/fs/win/store/WinStore.h>
#include <filesystem>
#include <iostream>
#include <memory>
#include <string>
#include "eden/fs/win/utils/Guid.h"
#include "eden/fs/win/utils/StringConv.h"
#include "gtest/gtest.h"

using namespace facebook::eden;

namespace {
class StateDbNodeTest : public ::testing::Test {
 protected:
  void SetUp() override {
    rootPath_ /= guid_.toString();
    rootKey_ = RegistryKey::createCurrentUser(rootPath_.c_str());
  }

  void TearDown() override {
    rootKey_.deleteKey();
  }

  Guid guid_{Guid::generate()};
  RegistryKey rootKey_;
  std::filesystem::path rootPath_{L"software\\facebook\\test"};
};
} // namespace

// TODO(puneetk): This could use more test.

TEST_F(StateDbNodeTest, testCreate) {
  const auto rootPath = rootPath_ / L"testStateDbNode";
  auto path = WinRelativePathW(L"dir1\\dir2\\file1");
  auto hash = Hash("5d105d15efb8b07a624be530ef2b62dab3bc2f8b");

  auto dbNode = StateDbNode(path, RegistryKey::createCurrentUser(path.c_str()));

  dbNode.setHash(hash);
  dbNode.setIsDirectory(true);
  dbNode.setEntryState(EntryState::CREATED);

  EXPECT_TRUE(dbNode.isDirectory());
  EXPECT_TRUE(dbNode.hasHash());
  EXPECT_EQ(dbNode.getHash(), hash);
  EXPECT_EQ(dbNode.getEntryState(), EntryState::CREATED);
}

TEST_F(StateDbNodeTest, testMove) {
  const auto rootPath = rootPath_ / L"testStateDbNode";
  auto path = WinRelativePathW(L"dir1\\dir2\\file1");
  auto hash = Hash("5d105d15efb8b07a624be530ef2b62dab3bc2f8b");

  auto dbNode = StateDbNode{path, RegistryKey::createCurrentUser(path.c_str())};

  dbNode.setHash(hash);
  dbNode.setIsDirectory(false);
  dbNode.setEntryState(EntryState::CREATED);

  auto dbNode2 = std::move(dbNode);

  EXPECT_FALSE(dbNode2.isDirectory());
  EXPECT_TRUE(dbNode2.hasHash());
  EXPECT_EQ(dbNode2.getHash(), hash);
  EXPECT_EQ(dbNode2.getEntryState(), EntryState::CREATED);
}

TEST_F(StateDbNodeTest, testDirEntries) {
  const auto rootPath = rootPath_ / L"testStateDbNode";
  auto path = WinRelativePathW(L"dir1\\dir2\\file1");
  auto hash = Hash("5d105d15efb8b07a624be530ef2b62dab3bc2f8b");

  auto dbNode = StateDbNode{path, RegistryKey::createCurrentUser(path.c_str())};

  dbNode.setHash(hash);
  dbNode.setIsDirectory(false);
  dbNode.setEntryState(EntryState::CREATED);

  auto dirEntry = dbNode.getDirectoryEntries();
  EXPECT_EQ(dirEntry.size(), 0);

  // getDirectoryEntries() has more test in CurrentStateTests.
}
