/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/fuse/FuseDispatcher.h"

#include <limits>

#include <folly/test/TestUtils.h>
#include <folly/testing/TestUtil.h>
#include <gtest/gtest.h>
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/InodeMap.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/store/ObjectFetchContext.h"
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
        ->mknod(
            kRootNodeId,
            kTooLong,
            S_IFREG | 0644,
            0,
            ObjectFetchContext::getNullContext())
        .get(0ms);
    FAIL() << "should throw";
  } catch (std::system_error& e) {
    EXPECT_EQ(ENAMETOOLONG, e.code().value());
  }
}

TEST_F(EdenDispatcherTest, mkdirReturnsNameTooLong) {
  try {
    mount.getDispatcher()
        ->mkdir(
            kRootNodeId,
            kTooLong,
            S_IFDIR | 0755,
            ObjectFetchContext::getNullContext())
        .get(0ms);
    FAIL() << "should throw";
  } catch (std::system_error& e) {
    EXPECT_EQ(ENAMETOOLONG, e.code().value());
  }
}

TEST_F(EdenDispatcherTest, symlinkReturnsNameTooLong) {
  try {
    mount.getDispatcher()
        ->symlink(
            kRootNodeId,
            kTooLong,
            "aoeu"_sp,
            ObjectFetchContext::getNullContext())
        .get(0ms);
    FAIL() << "should throw";
  } catch (std::system_error& e) {
    EXPECT_EQ(ENAMETOOLONG, e.code().value());
  }
}

TEST_F(EdenDispatcherTest, renameReturnsNameTooLong) {
  try {
    mount.getDispatcher()
        ->rename(
            kRootNodeId,
            "oldname"_pc,
            kRootNodeId,
            kTooLong,
            ObjectFetchContext::getNullContext())
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
        ->create(
            kRootNodeId,
            kTooLong,
            S_IFREG | 0644,
            0,
            ObjectFetchContext::getNullContext())
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
          ->lookup(
              0, kRootNodeId, "good"_pc, ObjectFetchContext::getNullContext())
          .get(0ms);
  EXPECT_NE(0u, entry.nodeid);
  EXPECT_NE(0, entry.attr.ino);
  EXPECT_EQ(entry.nodeid, entry.attr.ino);
}

TEST(RawEdenDispatcherTest, lookup_updates_last_used_time) {
  FakeTreeBuilder builder;
  builder.setFile("hello", "world");
  TestMount mount{builder};

  // Lookup to load the inode and get its number
  auto entry =
      mount.getDispatcher()
          ->lookup(
              0, kRootNodeId, "hello"_pc, ObjectFetchContext::getNullContext())
          .get(0ms);
  auto ino = InodeNumber{entry.nodeid};
  auto inode = mount.getEdenMount()->getInodeMap()->lookupInode(ino).get(0ms);
  auto timeAfterLookup = inode->getLastFsRequestTime();

  // Advance the clock
  mount.getClock().advance(60s);

  // Another lookup should update lastFsRequestTime
  mount.getDispatcher()
      ->lookup(0, kRootNodeId, "hello"_pc, ObjectFetchContext::getNullContext())
      .get(0ms);
  auto timeAfterSecondLookup = inode->getLastFsRequestTime();
  EXPECT_GT(
      timeAfterSecondLookup.toTimespec().tv_sec,
      timeAfterLookup.toTimespec().tv_sec);
}

TEST(RawEdenDispatcherTest, getattr_updates_last_used_time) {
  FakeTreeBuilder builder;
  builder.setFile("hello", "world");
  TestMount mount{builder};

  // Lookup to load the inode
  auto entry =
      mount.getDispatcher()
          ->lookup(
              0, kRootNodeId, "hello"_pc, ObjectFetchContext::getNullContext())
          .get(0ms);
  auto ino = InodeNumber{entry.nodeid};
  auto inode = mount.getEdenMount()->getInodeMap()->lookupInode(ino).get(0ms);

  // Advance the clock
  mount.getClock().advance(60s);

  // getattr should update lastFsRequestTime
  auto timeBefore = inode->getLastFsRequestTime();
  mount.getDispatcher()
      ->getattr(ino, ObjectFetchContext::getNullContext())
      .get(0ms);
  auto timeAfter = inode->getLastFsRequestTime();
  EXPECT_GT(timeAfter.toTimespec().tv_sec, timeBefore.toTimespec().tv_sec);
}

TEST(RawEdenDispatcherTest, lookup_returns_infinite_ttl_without_pressure_gc) {
  FakeTreeBuilder builder;
  builder.setFile("hello", "world");
  TestMount mount{builder};

  auto entry =
      mount.getDispatcher()
          ->lookup(
              0, kRootNodeId, "hello"_pc, ObjectFetchContext::getNullContext())
          .get(0ms);
  // Without pressure-based GC, TTL should be the default infinite value
  EXPECT_EQ(
      static_cast<uint64_t>(std::numeric_limits<int32_t>::max()),
      entry.entry_valid);
  EXPECT_EQ(
      static_cast<uint64_t>(std::numeric_limits<int32_t>::max()),
      entry.attr_valid);
}

TEST(RawEdenDispatcherTest, lookup_returns_dynamic_ttl_with_pressure_gc) {
  FakeTreeBuilder builder;
  builder.setFile("hello", "world");
  TestMount mount{builder};

  // Enable pressure-based GC with known settings
  mount.updateEdenConfig({
      {"experimental:enable-pressure-based-gc", "true"},
      {"mount:gc-pressure-min-inodes", "10"},
      {"mount:gc-pressure-max-inodes", "10000"},
      {"mount:fuse-ttl-max-seconds", "3600"},
      {"mount:fuse-ttl-min-seconds", "1"},
  });
  mount.getEdenMount()->updateInodePressurePolicy();

  auto entry =
      mount.getDispatcher()
          ->lookup(
              0, kRootNodeId, "hello"_pc, ObjectFetchContext::getNullContext())
          .get(0ms);
  // With very few inodes (below min of 10), TTL should be max value of 3600
  EXPECT_EQ(3600u, entry.entry_valid);
  EXPECT_EQ(3600u, entry.attr_valid);
}

TEST(RawEdenDispatcherTest, getattr_returns_dynamic_ttl_with_pressure_gc) {
  FakeTreeBuilder builder;
  builder.setFile("hello", "world");
  TestMount mount{builder};

  mount.updateEdenConfig({
      {"experimental:enable-pressure-based-gc", "true"},
      {"mount:gc-pressure-min-inodes", "10"},
      {"mount:gc-pressure-max-inodes", "10000"},
      {"mount:fuse-ttl-max-seconds", "3600"},
      {"mount:fuse-ttl-min-seconds", "1"},
  });
  mount.getEdenMount()->updateInodePressurePolicy();

  // First lookup to get the inode number
  auto entry =
      mount.getDispatcher()
          ->lookup(
              0, kRootNodeId, "hello"_pc, ObjectFetchContext::getNullContext())
          .get(0ms);

  // Now getattr on that inode
  auto attr =
      mount.getDispatcher()
          ->getattr(
              InodeNumber{entry.nodeid}, ObjectFetchContext::getNullContext())
          .get(0ms);
  EXPECT_EQ(3600u, attr.timeout_seconds);
}

TEST(RawEdenDispatcherTest, lookup_returns_valid_inode_for_bad_file) {
  FakeTreeBuilder builder;
  builder.setFile("bad", "contents");
  TestMount mount{builder, /*startReady=*/false};
  auto entryFuture =
      mount.getDispatcher()
          ->lookup(
              0, kRootNodeId, "bad"_pc, ObjectFetchContext::getNullContext())
          .semi()
          .via(mount.getServerExecutor().get());
  builder.getStoredBlob("bad"_relpath)
      ->triggerError(std::runtime_error("failed to load"));
  builder.setAllReady();
  mount.drainServerExecutor();
  auto entry = std::move(entryFuture).get(0ms);
  EXPECT_NE(0, entry.nodeid);
  EXPECT_NE(0, entry.attr.ino);
  EXPECT_EQ(entry.nodeid, entry.attr.ino);
}

#endif
