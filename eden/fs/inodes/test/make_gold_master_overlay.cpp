/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/init/Init.h>
#include <gflags/gflags.h>
#include <cstdlib>

#include "eden/common/telemetry/NullStructuredLogger.h"
#include "eden/common/utils/CaseSensitivity.h"
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/inodes/DirEntry.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/inodes/OverlayFile.h"
#include "eden/fs/telemetry/EdenStats.h"

using namespace facebook::eden;
using namespace folly::string_piece_literals;

DEFINE_string(
    overlayPath,
    "",
    "Directory where the gold master overlay is created");

/**
 * Create a small gold master overlay at the current version (v2) to
 * ensure that our code continues to be able to read it.
 *
 * The given overlayPath should not exist.
 */
void createGoldMasterOverlay(AbsolutePath overlayPath) {
  struct stat overlayStat;
  XCHECK_EQ(-1, stat(overlayPath.c_str(), &overlayStat))
      << fmt::format("given overlay path {} already exists", overlayPath);
  XCHECK_EQ(ENOENT, errno) << "error must be ENOENT";

  ObjectId id1{folly::ByteRange{"abcdabcdabcdabcdabcd"_sp}};
  ObjectId id2{folly::ByteRange{"01234012340123401234"_sp}};
  ObjectId id3{folly::ByteRange{"e0e0e0e0e0e0e0e0e0e0"_sp}};
  ObjectId id4{folly::ByteRange{"44444444444444444444"_sp}};

  auto overlay = Overlay::create(
      overlayPath,
      CaseSensitivity::Sensitive,
      InodeCatalogType::Legacy,
      kDefaultInodeCatalogOptions,
      std::make_shared<NullStructuredLogger>(),
      makeRefPtr<EdenStats>(),
      true,
      *EdenConfig::createTestEdenConfig());

  auto fileInode = overlay->allocateInodeNumber();
  XCHECK_EQ(2_ino, fileInode);
  auto subdirInode = overlay->allocateInodeNumber();
  auto emptyDirInode = overlay->allocateInodeNumber();
  auto helloInode = overlay->allocateInodeNumber();

  DirContents root(CaseSensitivity::Sensitive);
  root.emplace("file"_pc, S_IFREG | 0644, fileInode, id1);
  root.emplace("subdir"_pc, S_IFDIR | 0755, subdirInode, id2);

  DirContents subdir(CaseSensitivity::Sensitive);
  subdir.emplace("empty"_pc, S_IFDIR | 0755, emptyDirInode, id3);
  subdir.emplace("hello"_pc, S_IFREG | 0644, helloInode, id4);

  DirContents emptyDir(CaseSensitivity::Sensitive);

  overlay->saveOverlayDir(kRootNodeId, root);
  overlay->saveOverlayDir(subdirInode, subdir);
  overlay->saveOverlayDir(emptyDirInode, emptyDir);

  overlay->createOverlayFile(fileInode, folly::ByteRange{"contents"_sp});
  overlay->createOverlayFile(helloInode, folly::ByteRange{"world"_sp});
}

int main(int argc, char* argv[]) {
  folly::init(&argc, &argv);

  if (FLAGS_overlayPath.empty()) {
    fprintf(stderr, "overlayPath is required");
    return 1;
  }

  auto overlayPath = normalizeBestEffort(FLAGS_overlayPath.c_str());
  createGoldMasterOverlay(overlayPath);

  return 0;
}
