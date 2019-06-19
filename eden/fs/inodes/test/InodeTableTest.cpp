/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/inodes/InodeTable.h"

#include <folly/chrono/Conv.h>
#include <folly/experimental/TestUtil.h>
#include <folly/test/TestUtils.h>
#include <gtest/gtest.h>

using namespace facebook::eden;
using std::chrono::system_clock;

namespace {
struct InodeTableTest : public ::testing::Test {
  InodeTableTest()
      : tmpDir{"eden_inodetable_"},
        tablePath{(tmpDir.path() / "test.inodes").string()} {}

  folly::test::TemporaryDirectory tmpDir;
  std::string tablePath;
};

struct Int {
  enum { VERSION = 0 };
  /* implicit */ Int(int v) : value(v) {}
  operator int() const {
    return value;
  }

  int value;
};
} // namespace

TEST_F(InodeTableTest, persists_record) {
  {
    auto inodeTable = InodeTable<Int>::open(tablePath);
    inodeTable->set(10_ino, 15);
  }

  auto inodeTable = InodeTable<Int>::open(tablePath);
  EXPECT_EQ(15, inodeTable->getOrThrow(10_ino));
}

namespace {
struct Small {
  enum { VERSION = 0 };
  uint64_t x;
};
struct Large {
  enum { VERSION = 0 };
  uint64_t x;
  uint64_t y;
};
} // namespace

TEST_F(InodeTableTest, fails_to_load_if_record_changes_size_without_migration) {
  {
    auto inodeTable = InodeTable<Small>::open(tablePath);
    inodeTable->set(1_ino, {1});
  }

  ASSERT_THROW({ InodeTable<Large>::open(tablePath); }, std::runtime_error);
}

namespace {
struct OldRecord {
  enum { VERSION = 0 };
  uint32_t x;
  uint32_t y;
};

struct NewRecord {
  enum { VERSION = 1 };

  explicit NewRecord(const OldRecord& old)
      : x{old.x}, y{old.y}, z{old.x + old.y} {}

  uint64_t x;
  uint64_t y;
  uint64_t z;
};
} // namespace

TEST_F(InodeTableTest, migrate_from_one_record_format_to_another) {
  {
    auto inodeTable = InodeTable<OldRecord>::open(tablePath);
    inodeTable->set(1_ino, {11, 22});
    inodeTable->set(2_ino, {100, 200});
  }

  {
    auto inodeTable = InodeTable<NewRecord>::open<OldRecord>(tablePath);
    auto one = inodeTable->getOrThrow(1_ino);
    auto two = inodeTable->getOrThrow(2_ino);

    EXPECT_EQ(11, one.x);
    EXPECT_EQ(22, one.y);
    EXPECT_EQ(33, one.z);
    EXPECT_EQ(100, two.x);
    EXPECT_EQ(200, two.y);
    EXPECT_EQ(300, two.z);
  }
}

namespace {
struct OldVersion {
  enum { VERSION = 0 };
  uint32_t x;
  uint32_t y;
};

struct NewVersion {
  enum { VERSION = 1 };

  explicit NewVersion(const OldVersion& old)
      : x{old.x + old.y}, y{old.x - old.y} {}

  uint32_t x;
  uint32_t y;
};
} // namespace

TEST_F(
    InodeTableTest,
    migrate_from_one_record_format_to_another_even_if_same_size) {
  {
    auto inodeTable = InodeTable<OldVersion>::open(tablePath);
    inodeTable->set(1_ino, {7, 3});
    inodeTable->set(2_ino, {60, 40});
  }

  {
    auto inodeTable = InodeTable<NewVersion>::open<OldVersion>(tablePath);
    auto one = inodeTable->getOrThrow(1_ino);
    auto two = inodeTable->getOrThrow(2_ino);

    EXPECT_EQ(10, one.x);
    EXPECT_EQ(4, one.y);
    EXPECT_EQ(100, two.x);
    EXPECT_EQ(20, two.y);
  }
}

TEST_F(InodeTableTest, populateIfNotSet) {
  auto inodeTable = InodeTable<Int>::open(tablePath);
  inodeTable->set(1_ino, 15);

  inodeTable->populateIfNotSet(1_ino, [&] { return 100; });
  inodeTable->populateIfNotSet(2_ino, [&] { return 101; });

  EXPECT_EQ(15, inodeTable->getOrThrow(1_ino));
  EXPECT_EQ(101, inodeTable->getOrThrow(2_ino));
}

TEST_F(InodeTableTest, setDefault) {
  auto inodeTable = InodeTable<Int>::open(tablePath);
  EXPECT_EQ(14, inodeTable->setDefault(1_ino, 14));
  EXPECT_EQ(14, inodeTable->setDefault(1_ino, 16));
}

// TEST(INodeTable, set) {}
// TEST(INodeTable, getOrThrow) {}
// TEST(INodeTable, getOptional) {}
// TEST(INodeTable, modifyOrThrow) {}
// TEST(INodeTable, freeInodes) {}
