/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/inodes/fscatalog/InodePath.h"

#include <gtest/gtest.h>
#include <cstring>

using namespace facebook::eden;

TEST(WalPathTest, defaultConstructedIsEmpty) {
  WalPath path;
  EXPECT_STREQ("", path.c_str());
}

TEST(WalPathTest, kMaxPathLengthAccountsForDotWalSuffix) {
  // WalPath adds the ".wal" suffix (4 bytes) to the InodePath layout.
  EXPECT_EQ(InodePath::kMaxPathLength + 4, WalPath::kMaxPathLength);
}

TEST(WalPathTest, rawDataIsWritable) {
  WalPath path;
  auto& raw = path.rawData();
  constexpr folly::StringPiece kExample{"ab/12345.wal"};
  std::memcpy(raw.data(), kExample.data(), kExample.size());
  raw[kExample.size()] = '\0';

  EXPECT_STREQ("ab/12345.wal", path.c_str());
}

TEST(WalPathTest, convertsToRelativePathPiece) {
  WalPath path;
  auto& raw = path.rawData();
  constexpr folly::StringPiece kExample{"ab/12345.wal"};
  std::memcpy(raw.data(), kExample.data(), kExample.size());
  raw[kExample.size()] = '\0';

  RelativePathPiece asPiece = path;
  EXPECT_EQ("ab/12345.wal", asPiece.view());
}

TEST(WalPathTest, fillsExactlyKMaxPathLengthMinusOneBytes) {
  // Boundary check: construct a path that exactly fills the buffer to
  // (kMaxPathLength - 1) bytes plus a null terminator. This is the
  // worst-case shape the production code can produce: "ff/<max-decimal
  // inode>.wal". The test guards against off-by-one regressions in the
  // buffer layout, c_str() bounds, and RelativePathPiece conversion if
  // kMaxPathLength or any of its constituent constants ever change.
  static constexpr size_t kFillLen = WalPath::kMaxPathLength - 1;

  WalPath path;
  auto& raw = path.rawData();

  // "ff/" + max-digit inode placeholder + ".wal", structurally valid as
  // a single-component RelativePathPiece (one separator, no leading
  // slash, no ".." segments).
  std::string filled;
  filled.reserve(kFillLen);
  filled.append("ff/");
  filled.append(kFillLen - 3 - 4, 'x');
  filled.append(".wal");
  ASSERT_EQ(kFillLen, filled.size());

  std::memcpy(raw.data(), filled.data(), kFillLen);
  raw[kFillLen] = '\0';

  // c_str() must report exactly kFillLen bytes — not under (early
  // truncation) and not over (read past the null terminator).
  EXPECT_EQ(kFillLen, std::strlen(path.c_str()));
  EXPECT_STREQ(filled.c_str(), path.c_str());

  // RelativePathPiece conversion must use the same length.
  RelativePathPiece asPiece = path;
  EXPECT_EQ(filled, asPiece.view());
}

#endif
