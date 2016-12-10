/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */

#include "eden/fs/store/ObjectStores.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/ObjectStore.h"

// #movefast: I'm not sure where the right place for the utilities in this file
// is to live yet.

namespace facebook {
namespace eden {

std::unique_ptr<Tree> getTreeForDirectory(
    RelativePathPiece file,
    const Tree* root,
    const IObjectStore* objectStore) {
  auto iter = file.paths();
  auto currentDirectory = std::make_unique<Tree>(*root);
  for (auto piece : file.paths()) {
    auto entry = currentDirectory->getEntryPtr(piece.basename());
    if (entry != nullptr && entry->getType() == TreeEntryType::TREE) {
      currentDirectory = objectStore->getTree(entry->getHash());
    } else {
      // TODO(mbolin): Consider providing feedback to the caller to distinguish
      // ENOENT type errors from ENOTDIR (though we can probably defer this
      // until someone needs it). See comments from simpkins on D4032817.
      return nullptr;
    }
  }
  return currentDirectory;
}

std::unique_ptr<TreeEntry> getEntryForFile(
    RelativePathPiece file,
    const Tree* root,
    const IObjectStore* objectStore) {
  auto entry = getEntryForPath(file, root, objectStore);
  if (entry != nullptr && entry->getType() == TreeEntryType::BLOB) {
    return entry;
  }
  return nullptr;
}

std::unique_ptr<TreeEntry> getEntryForPath(
    RelativePathPiece file,
    const Tree* root,
    const IObjectStore* objectStore) {
  auto parentTree = getTreeForDirectory(file.dirname(), root, objectStore);
  if (parentTree != nullptr) {
    auto treeEntry = parentTree->getEntryPtr(file.basename());
    if (treeEntry != nullptr) {
      return std::make_unique<TreeEntry>(*treeEntry);
    }
  }
  return nullptr;
}
}
}
