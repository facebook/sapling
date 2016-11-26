/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */

#include "EdenMounts.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fuse/MountPoint.h"
#include "eden/utils/PathFuncs.h"

namespace facebook {
namespace eden {

std::unique_ptr<Tree> getRootTreeForMountPoint(
    fusell::MountPoint* mountPoint,
    ObjectStore* objectStore) {
  auto rootAsDirInode = mountPoint->getRootInode();
  auto rootAsTreeInode = std::dynamic_pointer_cast<TreeInode>(rootAsDirInode);
  {
    auto dir = rootAsTreeInode->getContents().rlock();
    auto& rootTreeHash = dir->treeHash.value();
    auto tree = objectStore->getTree(rootTreeHash);
    return tree;
  }
}

void getModifiedDirectoriesRecursive(
    RelativePathPiece dirPath,
    TreeInode* dir,
    std::vector<RelativePath>* modifiedDirectories) {
  dir->getContents().withRLock([&](const auto& contents) mutable {
    if (!contents.materialized) {
      return;
    }

    modifiedDirectories->push_back(dirPath.copy());
    for (auto& entIter : contents.entries) {
      const auto& ent = entIter.second;
      if (S_ISDIR(ent->mode) && ent->materialized) {
        const auto& name = entIter.first;
        auto childInode = dir->lookupChildByNameLocked(&contents, name);
        auto childPath = dirPath + name;
        auto childDir = std::dynamic_pointer_cast<TreeInode>(childInode);
        DCHECK(childDir->getContents().rlock()->materialized)
            << (dirPath + name) << " entry " << ent.get()
            << " materialized is true, but the contained dir is !materialized";

        getModifiedDirectoriesRecursive(
            childPath, childDir.get(), modifiedDirectories);
      }
    }
  });
}

// This function is not a method of MountPoint because it has a dependency on
// TreeInode. If MountPoint depended on TreeInode, it would create a circular
// dependency, which is why this function lives here.
std::unique_ptr<std::vector<RelativePath>> getModifiedDirectoriesForMount(
    fusell::MountPoint* mountPoint) {
  auto inodeDispatcher = mountPoint->getDispatcher();
  auto rootInode = inodeDispatcher->getDirInode(FUSE_ROOT_ID);
  auto treeInode = std::dynamic_pointer_cast<TreeInode>(rootInode);
  if (treeInode) {
    auto modifiedDirectories = std::make_unique<std::vector<RelativePath>>();
    getModifiedDirectoriesRecursive(
        RelativePathPiece(), treeInode.get(), modifiedDirectories.get());
    return modifiedDirectories;
  } else {
    throw std::runtime_error(folly::to<std::string>(
        "Could not find root TreeInode for ", mountPoint->getPath()));
  }
}
}
}
