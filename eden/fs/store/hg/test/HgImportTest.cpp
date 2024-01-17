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
    HgImporter importer(repo.path(), makeRefPtr<EdenStats>());
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
  EdenStatsPtr stats_ = makeRefPtr<EdenStats>();
};

} // namespace

#define EXPECT_BLOB_EQ(blob, data) \
  EXPECT_EQ((blob)->getContents().clone()->moveToFbString(), (data))

// TODO(T33797958): Check hg_importer_helper's exit code on Windows (in
// HgImportTest).
#ifndef _WIN32
TEST_F(HgImportTest, importerHelperExitsCleanly) {
  if (!testEnvironmentSupportsHg()) {
    GTEST_SKIP();
  }

  HgImporter importer(repo_.path(), stats_.copy());
  auto status = importer.debugStopHelperProcess();
  EXPECT_EQ(status.str(), "exited with status 0");
}
#endif
