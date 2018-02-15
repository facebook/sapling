/*
 *  Copyright (c) 2018-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
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

TEST(MappedDiskVector, grows_file) {
  auto tmpDir = TemporaryDirectory("eden_mdv_");
  auto tempPath = tmpDir.path() / "test.mdv";
  MappedDiskVector<uint64_t> mdv{tempPath.string()};
  EXPECT_EQ(0, mdv.size());

  struct stat st;
  ASSERT_EQ(0, stat(tempPath.c_str(), &st));
  auto old_size = st.st_size;

  // 8 MB
  constexpr uint64_t N = 1000000;
  for (uint64_t i = 0; i < N; ++i) {
    mdv.emplace_back(i);
  }
  EXPECT_EQ(N, mdv.size());

  ASSERT_EQ(0, stat(tempPath.c_str(), &st));
  auto new_size = st.st_size;
  EXPECT_GT(new_size, old_size);
}

TEST(MappedDiskVector, remembers_contents_on_reopen) {
  auto tmpDir = TemporaryDirectory("eden_mdv_");
  auto tempPath = tmpDir.path() / "test.mdv";
  {
    MappedDiskVector<uint64_t> mdv{tempPath.string()};
    mdv.emplace_back(15ull);
    mdv.emplace_back(25ull);
    mdv.emplace_back(35ull);
  }

  MappedDiskVector<uint64_t> mdv{tempPath.string()};
  EXPECT_EQ(3, mdv.size());
  EXPECT_EQ(15, mdv[0]);
  EXPECT_EQ(25, mdv[1]);
  EXPECT_EQ(35, mdv[2]);
}

TEST(MappedDiskVector, pop_back) {
  auto tmpDir = TemporaryDirectory("eden_mdv_");
  auto tempPath = tmpDir.path() / "test.mdv";
  MappedDiskVector<uint64_t> mdv{tempPath.string()};
  mdv.emplace_back(1ull);
  mdv.emplace_back(2ull);
  mdv.pop_back();
  mdv.emplace_back(3ull);
  EXPECT_EQ(2, mdv.size());
  EXPECT_EQ(1, mdv[0]);
  EXPECT_EQ(3, mdv[1]);
}
