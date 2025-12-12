/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/Range.h>
#include <folly/logging/xlog.h>
#include <gtest/gtest.h>
#include <memory>

#include "eden/common/utils/IDGen.h"
#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/TestOps.h"
#include "eden/fs/store/sl/SaplingObjectId.h"
#include "eden/fs/telemetry/EdenStats.h"

using namespace facebook::eden;

TEST(SlOidTest, test_moved_from_and_empty_hash_compare_the_same) {
  EdenStats stats;
  SlOid from{
      kEmptySha1,
      RelativePathPiece{"this is a long enough string to push past SSO"}};
  SlOid{std::move(from)};

  // @lint-ignore CLANGTIDY bugprone-use-after-move
  EXPECT_EQ(SlOid{}.path(), from.path());
  EXPECT_EQ(SlOid{}.node(), from.node());

  SlOid zero{kZeroHash, RelativePathPiece{}};
  EXPECT_EQ(SlOid{}.path(), zero.path());
  EXPECT_EQ(SlOid{}.node(), zero.node());
}

TEST(SlOidTest, round_trip_version_1) {
  EdenStats stats;
  Hash20 hash{folly::StringPiece{"0123456789abcdef0123456789abcdef01234567"}};

  {
    auto proxy1 = SlOid{hash, RelativePathPiece{}};
    EXPECT_EQ(hash, proxy1.node());
    EXPECT_EQ(RelativePathPiece{}, proxy1.path());
  }
  {
    auto proxy2 = SlOid{hash, RelativePathPiece{"some/longish/path"}};
    EXPECT_EQ(hash, proxy2.node());
    EXPECT_EQ(RelativePathPiece{"some/longish/path"}, proxy2.path());
  }
}

TEST(SlOidTest, round_trip_version_2) {
  EdenStats stats;
  Hash20 hash{folly::StringPiece{"0123456789abcdef0123456789abcdef01234567"}};

  auto proxy = SlOid{hash};
  EXPECT_EQ(hash, proxy.node());
  EXPECT_EQ(RelativePathPiece{}, proxy.path());
}
