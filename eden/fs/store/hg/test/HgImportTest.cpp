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
  std::shared_ptr<HgImporterThreadStats> stats_ =
      std::make_shared<HgImporterThreadStats>();
};

} // namespace

#define EXPECT_BLOB_EQ(blob, data) \
  EXPECT_EQ((blob)->getContents().clone()->moveToFbString(), (data))

TEST_F(HgImportTest, importTest) {
  // Set up the initial commit
  repo_.mkdir("foo");
  StringPiece barData = "this is a test file\n";
  repo_.writeFile("foo/bar.txt", barData);
  repo_.hg("add");
  auto commit1 = repo_.commit("Initial commit");

  // Import the root tree
  HgImporter importer(repo_.path(), &localStore_, stats_);

  // Test importing objects that do not exist
  Hash noSuchHash = makeTestHash("123");
  EXPECT_THROW_RE(
      importer.importFileContents(noSuchHash),
      std::exception,
      "value not present in store");

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
