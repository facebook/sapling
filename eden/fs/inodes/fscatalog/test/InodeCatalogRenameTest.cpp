/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/Portability.h>
#include <gtest/gtest.h>

#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/inodes/InodeCatalog.h"
#include "eden/fs/inodes/fscatalog/EphemeralFsInodeCatalog.h"
#include "eden/fs/inodes/overlay/gen-cpp2/overlay_types.h"

using namespace facebook::eden;
using namespace facebook::eden::path_literals;

namespace {

overlay::OverlayEntry makeDirectoryEntry(uint64_t ino) {
  overlay::OverlayEntry entry;
  entry.mode() = S_IFDIR | 0755;
  entry.inodeNumber() = static_cast<int64_t>(ino);
  return entry;
}

template <typename Catalog>
void addTwoEmptyDirectories(Catalog& catalog) {
  catalog.saveOverlayDir(InodeNumber{10}, overlay::OverlayDir{});
  catalog.saveOverlayDir(InodeNumber{11}, overlay::OverlayDir{});
  catalog.addChild(InodeNumber{10}, "e"_pc, makeDirectoryEntry(11));
  catalog.saveOverlayDir(InodeNumber{12}, overlay::OverlayDir{});
  catalog.addChild(InodeNumber{10}, "d"_pc, makeDirectoryEntry(12));
}

} // namespace

TEST(EphemeralFsInodeCatalogRenameTest, directoryOverEmptyDirectory) {
  EphemeralFsInodeCatalog catalog{nullptr};
  addTwoEmptyDirectories(catalog);
  EXPECT_THROW(
      catalog.renameChild(InodeNumber{10}, InodeNumber{10}, "e"_pc, "d"_pc),
      NonEmptyError);
  auto parent = catalog.loadOverlayDir(InodeNumber{10});
  ASSERT_TRUE(parent.has_value());
  EXPECT_EQ(2, parent->entries()->size());
}

TEST(EphemeralFsInodeCatalogRenameTest, intoParentWithoutRecord) {
  if (!folly::kIsSanitizeAddress) {
    GTEST_SKIP() << "the use after free is only detected under ASAN";
  }
  GTEST_FLAG_SET(death_test_style, "threadsafe");
  EXPECT_DEATH(
      {
        EphemeralFsInodeCatalog catalog{nullptr};
        catalog.saveOverlayDir(InodeNumber{100}, overlay::OverlayDir{});
        overlay::OverlayDir source;
        source.entries()->emplace("x", makeDirectoryEntry(2000));
        catalog.saveOverlayDir(InodeNumber{1000}, std::move(source));
        catalog.renameChild(
            InodeNumber{1000}, InodeNumber{3000}, "x"_pc, "y"_pc);
      },
      "AddressSanitizer");
}
