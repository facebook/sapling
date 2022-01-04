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
#include "eden/fs/store/MemoryLocalStore.h"
#include "eden/fs/store/hg/HgProxyHash.h"
#include "eden/fs/utils/IDGen.h"
#include "eden/fs/utils/PathFuncs.h"

using namespace facebook::eden;

TEST(HgProxyHashTest, testCopyMove) {
  auto store = std::make_shared<MemoryLocalStore>();
  ObjectId hash1, hash2;
  {
    auto write = store->beginWrite();
    hash1 = HgProxyHash::store(
        RelativePathPiece{"foobar"},
        Hash20{folly::StringPiece{"1111111111111111111111111111111111111111"}},
        write.get());

    hash2 = HgProxyHash::store(
        RelativePathPiece{"barfoo"},
        Hash20{folly::StringPiece{"DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD"}},
        write.get());

    write->flush();
  }
  auto orig1 = HgProxyHash::load(store.get(), hash1, "test");
  auto orig2 = HgProxyHash::load(store.get(), hash2, "test");
  auto second = orig1;

  EXPECT_EQ(orig1.path(), second.path());
  EXPECT_EQ(orig1.revHash(), second.revHash());

  second = orig2;
  EXPECT_EQ(orig2.path(), second.path());
  EXPECT_EQ(orig2.revHash(), second.revHash());

  auto moved = std::move(second);

  EXPECT_EQ(moved.path(), orig2.path());
  EXPECT_EQ(moved.revHash(), orig2.revHash());

  moved = std::move(orig1);

  EXPECT_EQ(moved.path(), RelativePathPiece{"foobar"});
  EXPECT_EQ(
      moved.revHash(),
      Hash20{folly::StringPiece{"1111111111111111111111111111111111111111"}});

  EXPECT_EQ(orig1.path(), RelativePathPiece{""});
  EXPECT_EQ(
      orig1.revHash(),
      Hash20{folly::StringPiece{"0000000000000000000000000000000000000000"}});
}

TEST(HgProxyHashTest, test_moved_from_and_empty_hash_compare_the_same) {
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
