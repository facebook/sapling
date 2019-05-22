/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include <folly/experimental/TestUtil.h>
#include <folly/futures/Future.h>
#include <folly/test/TestUtils.h>
#include <gmock/gmock.h>
#include <gtest/gtest.h>

#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/MemoryLocalStore.h"
#include "eden/fs/store/hg/HgImportPyError.h"
#include "eden/fs/store/hg/HgImporter.h"
#include "eden/fs/testharness/HgRepo.h"
#include "eden/fs/testharness/TestUtil.h"
#include "eden/fs/tracing/EdenStats.h"
#include "eden/fs/utils/PathFuncs.h"

using namespace facebook::eden;
using namespace std::chrono_literals;
using folly::StringPiece;
using folly::test::TemporaryDirectory;
using std::vector;
using testing::ElementsAre;

namespace {

class HgImportTest : public ::testing::Test {
 public:
  HgImportTest() {
    // Create the test repository
    repo_.hgInit();
    repo_.enableTreeManifest(testPath_ + "cache"_pc);
  }

 protected:
  TemporaryDirectory testDir_{"eden_hg_import_test"};
  AbsolutePath testPath_{testDir_.path().string()};
  HgRepo repo_{testPath_ + "repo"_pc};
  MemoryLocalStore localStore_;
  std::shared_ptr<EdenThreadStats> stats_ = std::make_shared<EdenThreadStats>();
};

} // namespace

#define EXPECT_BLOB_EQ(blob, data) \
  EXPECT_EQ((blob)->getContents().clone()->moveToFbString(), (data))

TEST_F(HgImportTest, importTest) {
  // Set up the initial commit
  repo_.mkdir("foo");
  StringPiece barData = "this is a test file\n";
  repo_.writeFile("foo/bar.txt", barData);
  StringPiece testData = "testing\n1234\ntesting\n";
  repo_.writeFile("foo/test.txt", testData);
  repo_.mkdir("src");
  repo_.mkdir("src/eden");
  StringPiece somelinkData = "this is the link contents";
  repo_.symlink(somelinkData, "src/somelink"_relpath);
  StringPiece mainData = "print('hello world\\n')\n";
  repo_.writeFile("src/eden/main.py", mainData, 0755);
  repo_.hg("add");
  auto commit1 = repo_.commit("Initial commit");

  // Import the root tree
  HgImporter importer(repo_.path(), &localStore_, stats_);
  auto rootTreeHash = importer.importFlatManifest(commit1.toString());
  auto rootTree = localStore_.getTree(rootTreeHash).get(10s);
  EXPECT_EQ(rootTreeHash, rootTree->getHash());
  EXPECT_EQ(rootTreeHash, rootTree->getHash());
  ASSERT_THAT(
      rootTree->getEntryNames(),
      ElementsAre(PathComponent{"foo"}, PathComponent{"src"}));

  // Get the "foo" tree.
  // When using flatmanifest, it should have already been imported
  // by importFlatManifest().
  auto fooEntry = rootTree->getEntryAt("foo"_pc);
  ASSERT_EQ(TreeEntryType::TREE, fooEntry.getType());
  auto fooTree = localStore_.getTree(fooEntry.getHash()).get(10s);
  ASSERT_TRUE(fooTree);
  ASSERT_THAT(
      fooTree->getEntryNames(),
      ElementsAre(PathComponent{"bar.txt"}, PathComponent{"test.txt"}));

  auto barEntry = fooTree->getEntryAt("bar.txt"_pc);
  ASSERT_EQ(TreeEntryType::REGULAR_FILE, barEntry.getType());
  auto testEntry = fooTree->getEntryAt("test.txt"_pc);
  ASSERT_EQ(TreeEntryType::REGULAR_FILE, testEntry.getType());

  // The blobs should not have been imported yet, though
  EXPECT_FALSE(localStore_.getBlob(barEntry.getHash()).get(0ms));
  EXPECT_FALSE(localStore_.getBlob(testEntry.getHash()).get(0ms));

  // Get the "src" tree from the LocalStore.
  auto srcEntry = rootTree->getEntryAt("src"_pc);
  ASSERT_EQ(TreeEntryType::TREE, srcEntry.getType());
  auto srcTree = localStore_.getTree(srcEntry.getHash()).get(10ms);
  ASSERT_TRUE(srcTree);
  ASSERT_THAT(
      srcTree->getEntryNames(),
      ElementsAre(PathComponent{"eden"}, PathComponent{"somelink"}));

  auto somelinkEntry = srcTree->getEntryAt("somelink"_pc);
  ASSERT_EQ(TreeEntryType::SYMLINK, somelinkEntry.getType());

  // Get the "src/eden" tree from the LocalStore
  auto edenEntry = srcTree->getEntryAt("eden"_pc);
  ASSERT_EQ(TreeEntryType::TREE, edenEntry.getType());
  auto edenTree = localStore_.getTree(edenEntry.getHash()).get(10s);
  ASSERT_TRUE(edenTree);
  ASSERT_THAT(edenTree->getEntryNames(), ElementsAre(PathComponent{"main.py"}));

  auto mainEntry = edenTree->getEntryAt("main.py"_pc);
  ASSERT_EQ(TreeEntryType::EXECUTABLE_FILE, mainEntry.getType());

  // Import and check the blobs
  auto barBlob = importer.importFileContents(barEntry.getHash());
  EXPECT_BLOB_EQ(barBlob, barData);

  auto testBlob = importer.importFileContents(testEntry.getHash());
  EXPECT_BLOB_EQ(testBlob, testData);

  auto mainBlob = importer.importFileContents(mainEntry.getHash());
  EXPECT_BLOB_EQ(mainBlob, mainData);

  auto somelinkBlob = importer.importFileContents(somelinkEntry.getHash());
  EXPECT_BLOB_EQ(somelinkBlob, somelinkData);

  // Test importing objects that do not exist
  Hash noSuchHash = makeTestHash("123");
  EXPECT_THROW_RE(
      importer.importFlatManifest(noSuchHash.toString()),
      HgImportPyError,
      "RepoLookupError: unknown revision");
  EXPECT_THROW_RE(
      importer.importFileContents(noSuchHash),
      std::exception,
      "value not present in store");

  // Test trying to import manifests using blob hashes, and vice-versa
  EXPECT_THROW_RE(
      importer.importFlatManifest(barEntry.getHash().toString()),
      HgImportPyError,
      "RepoLookupError: unknown revision");
  EXPECT_THROW_RE(
      importer.importFileContents(commit1),
      std::exception,
      "value not present in store");
}

// TODO(T33797958): Check hg_importer_helper's exit code on Windows (in
// HgImportTest).
#ifndef _WIN32
TEST_F(HgImportTest, importerHelperExitsCleanly) {
  HgImporter importer(repo_.path(), &localStore_, stats_);
  auto status = importer.debugStopHelperProcess();
  EXPECT_EQ(status.str(), "exited with status 0");
}
#endif
