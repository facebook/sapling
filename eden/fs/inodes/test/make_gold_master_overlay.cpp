/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/init/Init.h>
#include <gflags/gflags.h>
#include <stdlib.h>
#include "eden/fs/inodes/DirEntry.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/inodes/OverlayFile.h"

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
  CHECK_EQ(-1, stat(overlayPath.c_str(), &overlayStat))
      << "given overlay path " << overlayPath << " already exists";
  CHECK_EQ(ENOENT, errno) << "error must be ENOENT";

  Hash hash1{folly::ByteRange{"abcdabcdabcdabcdabcd"_sp}};
  Hash hash2{folly::ByteRange{"01234012340123401234"_sp}};
  Hash hash3{folly::ByteRange{"e0e0e0e0e0e0e0e0e0e0"_sp}};
  Hash hash4{folly::ByteRange{"44444444444444444444"_sp}};

  auto overlay = Overlay::create(overlayPath);

  auto fileInode = overlay->allocateInodeNumber();
  CHECK_EQ(2_ino, fileInode);
  auto subdirInode = overlay->allocateInodeNumber();
  auto emptyDirInode = overlay->allocateInodeNumber();
  auto helloInode = overlay->allocateInodeNumber();

  DirContents root;
  root.emplace("file"_pc, S_IFREG | 0644, fileInode, hash1);
  root.emplace("subdir"_pc, S_IFDIR | 0755, subdirInode, hash2);

  DirContents subdir;
  subdir.emplace("empty"_pc, S_IFDIR | 0755, emptyDirInode, hash3);
  subdir.emplace("hello"_pc, S_IFREG | 0644, helloInode, hash4);

  DirContents emptyDir;

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
