/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/EdenDispatcher.h"

#include <folly/experimental/TestUtil.h>
#include <folly/test/TestUtils.h>
#include <gtest/gtest.h>
#include "eden/fs/model/Blob.h"
#include "eden/fs/store/IObjectStore.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/StoredObject.h"
#include "eden/fs/testharness/TestMount.h"

using namespace facebook::eden;
using namespace std::chrono_literals;
using namespace folly::string_piece_literals;

namespace {
struct EdenDispatcherTest : ::testing::Test {
  EdenDispatcherTest() : mount{FakeTreeBuilder{}} {}
  TestMount mount;
};

constexpr auto kTooLongPiece = folly::StringPiece{
    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"};
static_assert(256 == kTooLongPiece.size(), "256 is one larger than the max!");

static const auto kTooLong = PathComponentPiece{kTooLongPiece};
} // namespace

TEST_F(EdenDispatcherTest, mknodReturnsNameTooLong) {
  try {
    mount.getDispatcher()
        ->mknod(kRootNodeId, kTooLong, S_IFREG | 0644, 0)
        .get(0ms);
    FAIL() << "should throw";
  } catch (std::system_error& e) {
    EXPECT_EQ(ENAMETOOLONG, e.code().value());
  }
}

TEST_F(EdenDispatcherTest, mkdirReturnsNameTooLong) {
  try {
    mount.getDispatcher()
        ->mkdir(kRootNodeId, kTooLong, S_IFDIR | 0755)
        .get(0ms);
    FAIL() << "should throw";
  } catch (std::system_error& e) {
    EXPECT_EQ(ENAMETOOLONG, e.code().value());
  }
}

TEST_F(EdenDispatcherTest, symlinkReturnsNameTooLong) {
  try {
    mount.getDispatcher()->symlink(kRootNodeId, kTooLong, "aoeu"_sp).get(0ms);
    FAIL() << "should throw";
  } catch (std::system_error& e) {
    EXPECT_EQ(ENAMETOOLONG, e.code().value());
  }
}

TEST_F(EdenDispatcherTest, renameReturnsNameTooLong) {
  try {
    mount.getDispatcher()
        ->rename(kRootNodeId, "oldname"_pc, kRootNodeId, kTooLong)
        .get(0ms);
    FAIL() << "should throw";
  } catch (std::system_error& e) {
    EXPECT_EQ(ENAMETOOLONG, e.code().value());
  }
}

TEST_F(EdenDispatcherTest, linkReturnsNameTooLong) {
  try {
    // Eden doesn't support hard links yet and this link call could never work
    // in the first place, but at least validate the target name length.
    mount.getDispatcher()->link(kRootNodeId, kRootNodeId, kTooLong).get(0ms);
    FAIL() << "should throw";
  } catch (std::system_error& e) {
    EXPECT_EQ(ENAMETOOLONG, e.code().value());
  }
}

TEST_F(EdenDispatcherTest, createReturnsNameTooLong) {
  try {
    mount.getDispatcher()
        ->create(kRootNodeId, kTooLong, S_IFREG | 0644, 0)
        .get(0ms);
    FAIL() << "should throw";
  } catch (std::system_error& e) {
    EXPECT_EQ(ENAMETOOLONG, e.code().value());
  }
}

TEST(RawEdenDispatcherTest, lookup_returns_valid_inode_for_good_file) {
  FakeTreeBuilder builder;
  builder.setFile("good", "contents");
  TestMount mount{builder};

  auto entry =
      mount.getDispatcher()
          ->lookup(kRootNodeId, "good"_pc, ObjectFetchContext::getNullContext())
          .get(0ms);
  EXPECT_NE(0, entry.nodeid);
  EXPECT_NE(0, entry.attr.ino);
  EXPECT_EQ(entry.nodeid, entry.attr.ino);
}

TEST(RawEdenDispatcherTest, lookup_returns_valid_inode_for_bad_file) {
  FakeTreeBuilder builder;
  builder.setFile("bad", "contents");
  TestMount mount{builder, /*startReady=*/false};
  auto entryFuture = mount.getDispatcher()->lookup(
      kRootNodeId, "bad"_pc, ObjectFetchContext::getNullContext());
  builder.getStoredBlob("bad"_relpath)
      ->triggerError(std::runtime_error("failed to load"));
  auto entry = std::move(entryFuture).get(0ms);
  EXPECT_NE(0, entry.nodeid);
  EXPECT_NE(0, entry.attr.ino);
  EXPECT_EQ(entry.nodeid, entry.attr.ino);
}
