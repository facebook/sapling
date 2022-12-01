/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/experimental/TestUtil.h>
#include <folly/futures/Future.h>
#include <folly/portability/GMock.h>
#include <folly/portability/GTest.h>
#include <folly/test/TestUtils.h>

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

TEST(HgImporter, ensure_HgImporter_is_linked_even_in_tsan) {
  if (testEnvironmentSupportsHg()) {
    TemporaryDirectory testDir{"eden_hg_import_test"};
    AbsolutePath testPath = canonicalPath(testDir.path().string());
    HgRepo repo{testPath + "repo"_pc};
    repo.hgInit(testPath + "cache"_pc);
    HgImporter importer(repo.path(), std::make_shared<EdenStats>());
  }
}

namespace {

class HgImportTest : public ::testing::Test {
 public:
  HgImportTest() {
    // Create the test repository
    repo_.hgInit(testPath_ + "cache"_pc);
  }

 protected:
  TemporaryDirectory testDir_{"eden_hg_import_test"};
  AbsolutePath testPath_ = canonicalPath(testDir_.path().string());
  HgRepo repo_{testPath_ + "repo"_pc};
  std::shared_ptr<EdenStats> stats_ = std::make_shared<EdenStats>();
};

} // namespace

#define EXPECT_BLOB_EQ(blob, data) \
  EXPECT_EQ((blob)->getContents().clone()->moveToFbString(), (data))

TEST_F(HgImportTest, importTest) {
  if (!testEnvironmentSupportsHg()) {
    GTEST_SKIP();
  }

  // Set up the initial commit
  repo_.mkdir("foo");
  StringPiece barData = "this is a test file\n";
  RelativePathPiece filePath{"foo/bar.txt"};
  repo_.writeFile(filePath, barData);
  repo_.hg("add", "foo");
  auto commit1 = repo_.commit("Initial commit");

  // Import the root tree
  HgImporter importer(repo_.path(), stats_);

  auto output = repo_.hg("manifest", "--debug");
  auto fileHash = output.substr(0, 40);

  auto blob = importer.importFileContents(filePath, Hash20{fileHash});
  EXPECT_BLOB_EQ(blob, barData);

  // Test importing objects that do not exist
  Hash20 noSuchHash = makeTestHash20("123");
  EXPECT_THROW(
      importer.importFileContents(filePath, noSuchHash), std::exception);

  EXPECT_THROW(
      importer.importFileContents(
          RelativePathPiece{"hello"}, Hash20{commit1.value()}),
      std::exception);
}

// TODO(T33797958): Check hg_importer_helper's exit code on Windows (in
// HgImportTest).
#ifndef _WIN32
TEST_F(HgImportTest, importerHelperExitsCleanly) {
  if (!testEnvironmentSupportsHg()) {
    GTEST_SKIP();
  }

  HgImporter importer(repo_.path(), stats_);
  auto status = importer.debugStopHelperProcess();
  EXPECT_EQ(status.str(), "exited with status 0");
}
#endif
