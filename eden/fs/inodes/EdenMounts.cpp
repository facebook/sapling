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

#include <boost/polymorphic_cast.hpp>
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/utils/PathFuncs.h"

namespace facebook {
namespace eden {

/*
 * TODO(t14009445): We should move this code into TreeInode, so that code
 * outside of TreeInode never needs to directly access the TreeInode contents_
 * and hold its lock.
 */
void getModifiedDirectoriesRecursive(
    RelativePathPiece dirPath,
    TreeInode* dir,
    const std::unordered_set<RelativePathPiece>* toIgnore,
    std::vector<RelativePath>& modifiedDirectories) {
  if (toIgnore->find(dirPath) != toIgnore->end()) {
    return;
  }

  dir->getContents().withRLock([&](const auto& contents) mutable {
    if (!contents.materialized) {
      return;
    }

    modifiedDirectories.push_back(dirPath.copy());
    for (auto& entIter : contents.entries) {
      const auto& ent = entIter.second;
      if (S_ISDIR(ent->mode) && ent->materialized) {
        const auto& name = entIter.first;
        auto childInode = ent->inode;
        CHECK(childInode != nullptr);
        auto childPath = dirPath + name;
        auto childDir = boost::polymorphic_downcast<TreeInode*>(childInode);
        DCHECK(childDir->getContents().rlock()->materialized)
            << (dirPath + name) << " entry " << ent.get()
            << " materialized is true, but the contained dir is !materialized";

        getModifiedDirectoriesRecursive(
            childPath, childDir, toIgnore, modifiedDirectories);
      }
    }
  });
}

std::vector<RelativePath> getModifiedDirectories(
    const EdenMount* mount,
    RelativePathPiece directoryInMount,
    const std::unordered_set<RelativePathPiece>* toIgnore) {
  auto tree = mount->getTreeInode(directoryInMount);
  std::vector<RelativePath> modifiedDirectories;
  getModifiedDirectoriesRecursive(
      directoryInMount, tree.get(), toIgnore, modifiedDirectories);
  return modifiedDirectories;
}

// This function is not a method of MountPoint because it has a dependency on
// TreeInode. If MountPoint depended on TreeInode, it would create a circular
// dependency, which is why this function lives here.
std::vector<RelativePath> getModifiedDirectoriesForMount(
    const EdenMount* mount,
    const std::unordered_set<RelativePathPiece>* toIgnore) {
  auto rootInode = mount->getRootInode();
  if (rootInode) {
    std::vector<RelativePath> modifiedDirectories;
    getModifiedDirectoriesRecursive(
        RelativePathPiece(), rootInode.get(), toIgnore, modifiedDirectories);
    return modifiedDirectories;
  } else {
    throw std::runtime_error(folly::to<std::string>(
        "Could not find root TreeInode for ", mount->getPath()));
  }
}
}
}
