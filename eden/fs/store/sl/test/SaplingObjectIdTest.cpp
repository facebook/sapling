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

TEST(SlOidTest, round_trip_with_path) {
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

TEST(SlOidTest, round_trip_without_path) {
  EdenStats stats;
  Hash20 hash{folly::StringPiece{"0123456789abcdef0123456789abcdef01234567"}};

  auto proxy = SlOid{hash};
  EXPECT_EQ(hash, proxy.node());
  EXPECT_EQ(RelativePathPiece{}, proxy.path());
}

TEST(SlOidViewTest, construct_from_objectid_with_path) {
  Hash20 hash{folly::StringPiece{"0123456789abcdef0123456789abcdef01234567"}};
  auto slOid = SlOid{hash, RelativePathPiece{"some/path"}};
  auto oid = std::move(slOid).oid();

  SlOidView view{oid};
  EXPECT_EQ(hash, view.node());
  EXPECT_EQ(RelativePathPiece{"some/path"}, view.path());
}

TEST(SlOidViewTest, construct_from_objectid_no_path) {
  Hash20 hash{folly::StringPiece{"0123456789abcdef0123456789abcdef01234567"}};
  auto slOid = SlOid{hash};
  auto oid = std::move(slOid).oid();

  SlOidView view{oid};
  EXPECT_EQ(hash, view.node());
  EXPECT_EQ(RelativePathPiece{}, view.path());
}

TEST(SlOidViewTest, construct_from_byte_range) {
  Hash20 hash{folly::StringPiece{"0123456789abcdef0123456789abcdef01234567"}};
  auto slOid = SlOid{hash, RelativePathPiece{"test/path"}};
  auto oid = std::move(slOid).oid();

  SlOidView view{oid.getBytes()};
  EXPECT_EQ(hash, view.node());
  EXPECT_EQ(RelativePathPiece{"test/path"}, view.path());
}

TEST(SlOidTest, format_without_path) {
  Hash20 hash{folly::StringPiece{"0123456789abcdef0123456789abcdef01234567"}};
  SaplingObjectId oid{hash};
  auto formatted = fmt::format("{}", oid);
  EXPECT_EQ("node=0123456789abcdef0123456789abcdef01234567", formatted);
}

TEST(SlOidTest, format_with_path) {
  Hash20 hash{folly::StringPiece{"0123456789abcdef0123456789abcdef01234567"}};
  SaplingObjectId oid{hash, RelativePathPiece{"some/path/to/file.txt"}};
  auto formatted = fmt::format("{}", oid);
  EXPECT_EQ(
      "node=0123456789abcdef0123456789abcdef01234567 path=some/path/to/file.txt",
      formatted);
}

TEST(SlOidTest, format_view_without_path) {
  Hash20 hash{folly::StringPiece{"0123456789abcdef0123456789abcdef01234567"}};
  SaplingObjectId oid{hash};
  ObjectId obj = std::move(oid).oid();
  SaplingObjectIdView view{obj};
  auto formatted = fmt::format("{}", view);
  EXPECT_EQ("node=0123456789abcdef0123456789abcdef01234567", formatted);
}

TEST(SlOidTest, format_view_with_path) {
  Hash20 hash{folly::StringPiece{"0123456789abcdef0123456789abcdef01234567"}};
  SaplingObjectId oid{hash, RelativePathPiece{"some/path/to/file.txt"}};
  ObjectId obj = std::move(oid).oid();
  SaplingObjectIdView view{obj};
  auto formatted = fmt::format("{}", view);
  EXPECT_EQ(
      "node=0123456789abcdef0123456789abcdef01234567 path=some/path/to/file.txt",
      formatted);
}

TEST(SlOidTest, format_with_empty_path) {
  Hash20 hash{folly::StringPiece{"0123456789abcdef0123456789abcdef01234567"}};
  SaplingObjectId oid{hash, RelativePathPiece{}};
  auto formatted = fmt::format("{}", oid);
  EXPECT_EQ("node=0123456789abcdef0123456789abcdef01234567 path=", formatted);
}

TEST(SlOidTest, format_view_with_empty_path) {
  Hash20 hash{folly::StringPiece{"0123456789abcdef0123456789abcdef01234567"}};
  SaplingObjectId oid{hash, RelativePathPiece{}};
  ObjectId obj = std::move(oid).oid();
  SaplingObjectIdView view{obj};
  auto formatted = fmt::format("{}", view);
  EXPECT_EQ("node=0123456789abcdef0123456789abcdef01234567 path=", formatted);
}
