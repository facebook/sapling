/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/model/hg/misc.h"
#include "eden/fs/store/testutil/FakeObjectStore.h"

#include <gtest/gtest.h>

using namespace facebook::eden;
using std::move;
using std::string;
using std::unique_ptr;
using std::unordered_map;
using std::vector;

namespace {

Hash rootTreeHash("1111111111111111111111111111111111111111");
Hash aFileHash("ffffffffffffffffffffffffffffffffffffffff");
Hash aDirHash("abcdabcdabcdabcdabcdabcdabcdabcdabcdabcd");
Hash deepFileHash("3333333333333333333333333333333333333333");
Hash deepDirHash("4444444444444444444444444444444444444444");
Hash middleDirHash("5555555555555555555555555555555555555555");

uint8_t rw_ = 0b110;
uint8_t rwx = 0b111;

unique_ptr<FakeObjectStore> createObjectStoreForTest(Hash& hashForRootTree);
}

TEST(getEntryForFile, specifyingAnEmptyFilePathDoesNotThrowAnException) {
  auto store = createObjectStoreForTest(rootTreeHash);
  auto rootTree = store->getTree(rootTreeHash).get();

  RelativePathPiece emptyPath("");
  auto noCorrespondingTreeEntry =
      getEntryForFile(emptyPath, rootTree, store.get());
  EXPECT_EQ(nullptr, noCorrespondingTreeEntry)
      << "Should be nullptr because "
      << "there is no file that corresponds to the empty string.";
}

TEST(getEntryForFile, fileEntryInRoot) {
  auto store = createObjectStoreForTest(rootTreeHash);
  auto rootTree = store->getTree(rootTreeHash).get();

  RelativePathPiece file("a_file");
  auto treeEntry = getEntryForFile(file, rootTree, store.get());
  ASSERT_NE(nullptr, treeEntry) << "There should be an entry for " << file;

  CHECK_EQ("a_file", treeEntry->getName());
  CHECK_EQ(aFileHash, treeEntry->getHash());

  RelativePathPiece notAFile("not_a_file");
  auto nonExistentTreeEntry = getEntryForFile(notAFile, rootTree, store.get());
  EXPECT_EQ(nullptr, nonExistentTreeEntry)
      << "Should be nullptr because not found.";
}

TEST(getEntryForFile, directoryEntryInRoot) {
  auto store = createObjectStoreForTest(rootTreeHash);
  auto rootTree = store->getTree(rootTreeHash).get();

  RelativePathPiece file("a_dir");
  auto treeEntry = getEntryForFile(file, rootTree, store.get());
  EXPECT_EQ(nullptr, treeEntry)
      << "Should be nullptr because a_dir is a directory, not a file.";

  RelativePathPiece notADir("not_a_dir");
  auto nonExistentTreeEntry = getEntryForFile(notADir, rootTree, store.get());
  EXPECT_EQ(nullptr, nonExistentTreeEntry)
      << "Should be nullptr because not found.";
}

TEST(getEntryForFile, fileEntryInDeepDirectory) {
  auto store = createObjectStoreForTest(rootTreeHash);
  auto rootTree = store->getTree(rootTreeHash).get();

  RelativePathPiece file("a_dir/deep_dir/deep_file");
  auto treeEntry = getEntryForFile(file, rootTree, store.get());
  ASSERT_NE(nullptr, treeEntry) << "There should be an entry for " << file;

  CHECK_EQ("deep_file", treeEntry->getName());
  CHECK_EQ(deepFileHash, treeEntry->getHash());
}

namespace {
unique_ptr<FakeObjectStore> createObjectStoreForTest(Hash& hashForRootTree) {
  FakeObjectStore store;

  vector<TreeEntry> deepDirEntries;
  deepDirEntries.emplace_back(
      deepFileHash, "deep_file", FileType::REGULAR_FILE, rw_);
  Tree deepTree(std::move(deepDirEntries), deepDirHash);
  store.addTree(std::move(deepTree));

  vector<TreeEntry> middleDirEntries;
  middleDirEntries.emplace_back(
      deepDirHash, "deep_dir", FileType::DIRECTORY, rwx);
  Tree middleTree(std::move(middleDirEntries), aDirHash);
  store.addTree(std::move(middleTree));

  vector<TreeEntry> rootEntries;
  rootEntries.emplace_back(aDirHash, "a_dir", FileType::DIRECTORY, rwx);
  rootEntries.emplace_back(aFileHash, "a_file", FileType::REGULAR_FILE, rw_);
  Tree rootTree(std::move(rootEntries), hashForRootTree);
  store.addTree(std::move(rootTree));

  return std::make_unique<FakeObjectStore>(store);
}
}
