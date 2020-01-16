/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/experimental/TestUtil.h>
#include <folly/futures/Future.h>
#include <folly/test/TestUtils.h>
#include <gmock/gmock.h>
#include <gtest/gtest.h>

#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/hg/HgImportPyError.h"
#include "eden/fs/store/hg/HgImporter.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/testharness/HgRepo.h"
#include "eden/fs/testharness/TestUtil.h"
#include "eden/fs/utils/PathFuncs.h"

using namespace facebook::eden;
using folly::StringPiece;
using folly::test::TemporaryDirectory;

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
  std::shared_ptr<EdenStats> stats_ = std::make_shared<EdenStats>();
};

} // namespace

#define EXPECT_BLOB_EQ(blob, data) \
  EXPECT_EQ((blob)->getContents().clone()->moveToFbString(), (data))

TEST_F(HgImportTest, importTest) {
  // Set up the initial commit
  repo_.mkdir("foo");
  StringPiece barData = "this is a test file\n";
  RelativePathPiece filePath{"foo/bar.txt"};
  repo_.writeFile(filePath, barData);
  repo_.hg("add");
  auto commit1 = repo_.commit("Initial commit");

  // Import the root tree
  HgImporter importer(repo_.path(), stats_);

  auto fileHash = repo_.hg("manifest", "--debug").substr(0, 40);

  auto blob = importer.importFileContents(filePath, Hash{fileHash});
  EXPECT_BLOB_EQ(blob, barData);

  // Test importing objects that do not exist
  Hash noSuchHash = makeTestHash("123");
  EXPECT_THROW_RE(
      importer.importFileContents(filePath, noSuchHash),
      std::exception,
      "no match found");

  EXPECT_THROW_RE(
      importer.importFileContents(RelativePathPiece{"hello"}, commit1),
      std::exception,
      "no match found");
}

// TODO(T33797958): Check hg_importer_helper's exit code on Windows (in
// HgImportTest).
#ifndef _WIN32
TEST_F(HgImportTest, importerHelperExitsCleanly) {
  HgImporter importer(repo_.path(), stats_);
  auto status = importer.debugStopHelperProcess();
  EXPECT_EQ(status.str(), "exited with status 0");
}
#endif
