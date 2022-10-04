/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/Range.h>
#include <folly/logging/xlog.h>
#include <folly/portability/GTest.h>
#include <memory>

#include "eden/fs/model/Hash.h"
#include "eden/fs/model/TestOps.h"
#include "eden/fs/store/hg/HgProxyHash.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/utils/IDGen.h"
#include "eden/fs/utils/PathFuncs.h"

using namespace facebook::eden;

TEST(HgProxyHashTest, test_moved_from_and_empty_hash_compare_the_same) {
  EdenStats stats;
  HgProxyHash from{
      RelativePathPiece{"this is a long enough string to push past SSO"},
      kEmptySha1};
  HgProxyHash{std::move(from)};

  EXPECT_EQ(HgProxyHash{}.path(), from.path());
  EXPECT_EQ(HgProxyHash{}.revHash(), from.revHash());
  EXPECT_EQ(HgProxyHash{}.sha1(), from.sha1());

  HgProxyHash zero{RelativePathPiece{}, kZeroHash};
  EXPECT_EQ(HgProxyHash{}.path(), zero.path());
  EXPECT_EQ(HgProxyHash{}.revHash(), zero.revHash());
  EXPECT_EQ(HgProxyHash{}.sha1(), zero.sha1());
}

TEST(HgProxyHashTest, round_trip_version_1) {
  EdenStats stats;
  Hash20 hash{folly::StringPiece{"0123456789abcdef0123456789abcdef01234567"}};

  {
    auto proxy1 = HgProxyHash::load(
        nullptr,
        HgProxyHash::makeEmbeddedProxyHash1(hash, RelativePathPiece{}),
        "test",
        stats);
    EXPECT_EQ(hash, proxy1.revHash());
    EXPECT_EQ(RelativePathPiece{}, proxy1.path());
  }
  {
    auto proxy2 = HgProxyHash::load(
        nullptr,
        HgProxyHash::makeEmbeddedProxyHash1(
            hash, RelativePathPiece{"some/longish/path"}),
        "test",
        stats);
    EXPECT_EQ(hash, proxy2.revHash());
    EXPECT_EQ(RelativePathPiece{"some/longish/path"}, proxy2.path());
  }
}

TEST(HgProxyHashTest, round_trip_version_2) {
  EdenStats stats;
  Hash20 hash{folly::StringPiece{"0123456789abcdef0123456789abcdef01234567"}};

  auto proxy = HgProxyHash::load(
      nullptr, HgProxyHash::makeEmbeddedProxyHash2(hash), "test", stats);
  EXPECT_EQ(hash, proxy.revHash());
  EXPECT_EQ(RelativePathPiece{}, proxy.path());
}
