/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/memcatalog/MemInodeCatalog.h"

#include <folly/portability/GTest.h>
#include <folly/portability/SysStat.h>

#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/inodes/InodeCatalog.h"
#include "eden/fs/inodes/overlay/gen-cpp2/overlay_types.h"

namespace facebook::eden {
namespace {

using namespace facebook::eden::path_literals;

constexpr InodeNumber kRoot{1};

overlay::OverlayEntry makeEntry(InodeNumber ino, mode_t mode) {
  overlay::OverlayEntry entry;
  entry.mode() = mode;
  entry.inodeNumber() = ino.get();
  return entry;
}

TEST(MemInodeCatalogTest, renameOverEmptyDirectory) {
  MemInodeCatalog catalog;
  catalog.addChild(
      kRoot, "empty"_pc, makeEntry(InodeNumber{2}, S_IFDIR | 0755));
  catalog.saveOverlayDir(InodeNumber{2}, overlay::OverlayDir{});
  catalog.addChild(kRoot, "src"_pc, makeEntry(InodeNumber{3}, S_IFDIR | 0755));

  catalog.renameChild(kRoot, kRoot, "src"_pc, "empty"_pc);

  EXPECT_FALSE(catalog.hasChild(kRoot, "src"_pc));
  auto root = catalog.loadOverlayDir(kRoot);
  ASSERT_TRUE(root.has_value());
  EXPECT_EQ(3u, *root->entries()->at("empty").inodeNumber());
}

TEST(MemInodeCatalogTest, renameOverNonEmptyDirectoryThrows) {
  MemInodeCatalog catalog;
  catalog.addChild(kRoot, "full"_pc, makeEntry(InodeNumber{2}, S_IFDIR | 0755));
  catalog.addChild(
      InodeNumber{2},
      "child.txt"_pc,
      makeEntry(InodeNumber{4}, S_IFREG | 0644));
  catalog.addChild(kRoot, "src"_pc, makeEntry(InodeNumber{3}, S_IFDIR | 0755));

  EXPECT_THROW(
      catalog.renameChild(kRoot, kRoot, "src"_pc, "full"_pc), NonEmptyError);
  EXPECT_TRUE(catalog.hasChild(kRoot, "src"_pc));
  auto root = catalog.loadOverlayDir(kRoot);
  ASSERT_TRUE(root.has_value());
  EXPECT_EQ(2u, *root->entries()->at("full").inodeNumber());
}

} // namespace
} // namespace facebook::eden
