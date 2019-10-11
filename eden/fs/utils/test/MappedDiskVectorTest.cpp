/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/MappedDiskVector.h"

#include <folly/experimental/TestUtil.h>
#include <folly/test/TestUtils.h>
#include <gtest/gtest.h>

using facebook::eden::MappedDiskVector;
using folly::test::TemporaryDirectory;

TEST(MappedDiskVector, roundUpToNonzeroPageSize) {
  using namespace facebook::eden::detail;
  EXPECT_EQ(kPageSize, roundUpToNonzeroPageSize(0));
  EXPECT_EQ(kPageSize, roundUpToNonzeroPageSize(1));
  EXPECT_EQ(kPageSize, roundUpToNonzeroPageSize(kPageSize - 1));
  EXPECT_EQ(kPageSize, roundUpToNonzeroPageSize(kPageSize));
  EXPECT_EQ(kPageSize * 2, roundUpToNonzeroPageSize(kPageSize + 1));
  EXPECT_EQ(kPageSize * 2, roundUpToNonzeroPageSize(kPageSize * 2 - 1));
  EXPECT_EQ(kPageSize * 2, roundUpToNonzeroPageSize(kPageSize * 2));
}

namespace {
struct MappedDiskVectorTest : ::testing::Test {
  MappedDiskVectorTest()
      : tmpDir{"eden_mdv_"}, mdvPath{(tmpDir.path() / "test.mdv").string()} {}
  TemporaryDirectory tmpDir;
  std::string mdvPath;
};

struct U64 {
  enum { VERSION = 0 };

  /* implicit */ U64(uint64_t v) : value{v} {}
  operator uint64_t() const {
    return value;
  }
  uint64_t value;
};
} // namespace

TEST_F(MappedDiskVectorTest, grows_file) {
  auto mdv = MappedDiskVector<U64>::open(mdvPath);
  EXPECT_EQ(0, mdv.size());

  struct stat st;
  ASSERT_EQ(0, stat(mdvPath.c_str(), &st));
  auto old_size = st.st_size;

  // 8 MB
  constexpr uint64_t N = 1000000;
  for (uint64_t i = 0; i < N; ++i) {
    mdv.emplace_back(i);
  }
  EXPECT_EQ(N, mdv.size());

  ASSERT_EQ(0, stat(mdvPath.c_str(), &st));
  auto new_size = st.st_size;
  EXPECT_GT(new_size, old_size);
}

TEST_F(MappedDiskVectorTest, remembers_contents_on_reopen) {
  {
    auto mdv = MappedDiskVector<U64>::open(mdvPath);
    mdv.emplace_back(15ull);
    mdv.emplace_back(25ull);
    mdv.emplace_back(35ull);
  }

  auto mdv = MappedDiskVector<U64>::open(mdvPath);
  EXPECT_EQ(3, mdv.size());
  EXPECT_EQ(15, mdv[0]);
  EXPECT_EQ(25, mdv[1]);
  EXPECT_EQ(35, mdv[2]);
}

TEST_F(MappedDiskVectorTest, pop_back) {
  auto mdv = MappedDiskVector<U64>::open(mdvPath);
  mdv.emplace_back(1ull);
  mdv.emplace_back(2ull);
  mdv.pop_back();
  mdv.emplace_back(3ull);
  EXPECT_EQ(2, mdv.size());
  EXPECT_EQ(1, mdv[0]);
  EXPECT_EQ(3, mdv[1]);
}

namespace {
struct Small {
  enum { VERSION = 0 };
  unsigned x;
};
struct Large {
  enum { VERSION = 0 };
  unsigned x;
  unsigned y;
};
struct SmallNew {
  enum { VERSION = 1 };
  unsigned x;
};
} // namespace

TEST_F(MappedDiskVectorTest, throws_if_size_does_not_match) {
  {
    auto mdv = MappedDiskVector<Small>::open(mdvPath);
    mdv.emplace_back(Small{1});
  }

  try {
    auto mdv = MappedDiskVector<Large>::open(mdvPath);
    FAIL() << "MappedDiskVector didn't throw";
  } catch (const std::runtime_error& e) {
    EXPECT_EQ(
        "Record size does not match size recorded in file. "
        "Expected 8 but file has 4",
        std::string(e.what()));
  } catch (const std::exception& e) {
    FAIL() << "Unexpected exception: " << e.what();
  }
}

TEST_F(MappedDiskVectorTest, throws_if_version_does_not_match) {
  {
    auto mdv = MappedDiskVector<Small>::open(mdvPath);
    mdv.emplace_back(Small{1});
  }

  try {
    auto mdv = MappedDiskVector<SmallNew>::open(mdvPath);
    FAIL() << "MappedDiskVector didn't throw";
  } catch (const std::runtime_error& e) {
    EXPECT_EQ(
        "Unexpected record size and version. "
        "Expected size=4, version=1 but got size=4, version=0",
        std::string(e.what()));
  } catch (const std::exception& e) {
    FAIL() << "Unexpected exception: " << e.what();
  }
}

namespace {
struct Old {
  enum { VERSION = 0 };
  unsigned x;
};
struct New {
  enum { VERSION = 1 };
  explicit New(const Old& old) : x(-old.x), y(old.x) {}
  unsigned x;
  unsigned y;
};
} // namespace

TEST_F(MappedDiskVectorTest, migrates_from_old_version_to_new) {
  {
    auto mdv = MappedDiskVector<Old>::open(mdvPath);
    mdv.emplace_back(Old{1});
    mdv.emplace_back(Old{2});
  }

  {
    auto mdv = MappedDiskVector<New>::open<Old>(mdvPath);
    EXPECT_EQ(2, mdv.size());
    EXPECT_EQ(-1, mdv[0].x);
    EXPECT_EQ(1, mdv[0].y);
    EXPECT_EQ(-2, mdv[1].x);
    EXPECT_EQ(2, mdv[1].y);
  }

  // and moves the new database over the old one
  {
    auto mdv = MappedDiskVector<New>::open(mdvPath);
    EXPECT_EQ(2, mdv.size());
    EXPECT_EQ(-1, mdv[0].x);
    EXPECT_EQ(1, mdv[0].y);
    EXPECT_EQ(-2, mdv[1].x);
    EXPECT_EQ(2, mdv[1].y);
  }
}

namespace {
struct V1 {
  enum { VERSION = 1 };
  uint8_t value;
  uint8_t conversionCount{0};
};
struct V2 {
  enum { VERSION = 2 };
  explicit V2(V1 old)
      : value(old.value), conversionCount(old.conversionCount + 1) {}
  uint16_t value;
  uint16_t conversionCount{0};
};
struct V3 {
  enum { VERSION = 3 };
  explicit V3(V2 old)
      : value(old.value), conversionCount(old.conversionCount + 1) {}
  uint32_t value;
  uint32_t conversionCount{0};
};
struct V4 {
  enum { VERSION = 4 };
  explicit V4(V3 old)
      : value(old.value), conversionCount(old.conversionCount + 1) {}
  uint64_t value;
  uint64_t conversionCount{0};
};
} // namespace

TEST_F(MappedDiskVectorTest, migrates_across_multiple_versions) {
  {
    auto mdv = MappedDiskVector<V1>::open(mdvPath);
    mdv.emplace_back(V1{1});
    mdv.emplace_back(V1{2});
  }

  {
    auto mdv = MappedDiskVector<V4>::open<V3, V2, V1>(mdvPath);
    EXPECT_EQ(1, mdv[0].value);
    EXPECT_EQ(3, mdv[0].conversionCount);
    EXPECT_EQ(2, mdv[1].value);
    EXPECT_EQ(3, mdv[1].conversionCount);
  }
}

TEST_F(MappedDiskVectorTest, migrate_overwrites_existing_tmp_file) {
  {
    auto mdv = MappedDiskVector<Old>::open(mdvPath);
    mdv.emplace_back(Old{1});
    mdv.emplace_back(Old{2});
  }

  folly::writeFileAtomic(mdvPath + ".tmp", "junk data");

  {
    auto mdv = MappedDiskVector<New>::open<Old>(mdvPath);
    EXPECT_EQ(2, mdv.size());
    EXPECT_EQ(-1, mdv[0].x);
    EXPECT_EQ(1, mdv[0].y);
    EXPECT_EQ(-2, mdv[1].x);
    EXPECT_EQ(2, mdv[1].y);
  }
}
