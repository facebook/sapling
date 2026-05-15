/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include <gtest/gtest.h>
#include <cstdint>

#include "eden/fs/inodes/fscatalog/FsInodeCatalog.h"
#include "eden/fs/inodes/fscatalog/InodePath.h"

using namespace facebook::eden;

TEST(
    FsInodeCatalogWalPathTest,
    getWalPath_producesShardedPathWithDotWalSuffix) {
  // Inode 0xab is sharded by its low byte, so the shard dir is "ab" and the
  // filename is its decimal form with the ".wal" suffix.
  auto path = FsFileContentStore::getWalPath(InodeNumber{0xab});
  EXPECT_STREQ("ab/171.wal", path.c_str());
}

TEST(FsInodeCatalogWalPathTest, getWalPath_handlesMaxUint64Inode) {
  // The maximum inode number must still fit within WalPath::kMaxPathLength.
  // Low byte of UINT64_MAX is 0xff so the shard dir is "ff".
  auto path = FsFileContentStore::getWalPath(InodeNumber{UINT64_MAX});
  EXPECT_STREQ("ff/18446744073709551615.wal", path.c_str());
}

#endif
