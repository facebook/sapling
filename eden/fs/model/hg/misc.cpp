/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */

#include "misc.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/ObjectStore.h"

// #movefast: I'm not sure where the right place for the utilities in this file
// is to live yet.

namespace facebook {
namespace eden {

const TreeEntry*
getEntryForFile(RelativePathPiece file, Tree* root, IObjectStore* objectStore) {
  auto iter = file.paths();
  Tree* currentDirectory = root;
  for (auto it = iter.begin(); it != iter.end();) {
    auto piece = it.piece();

    auto entry = currentDirectory->getEntryPtr(piece.basename());
    if (entry == nullptr) {
      return nullptr;
    }

    ++it; // Advance the iterator to see if piece is the last item in the iter.
    if (it != iter.end()) {
      // We are still traversing the chain of directories.
      if (entry->getType() != TreeEntryType::TREE) {
        return nullptr;
      }
      currentDirectory = objectStore->getTree(entry->getHash()).get();
    } else {
      // This should be the last path component, so it should correspond
      // to a file.
      if (entry->getType() != TreeEntryType::BLOB) {
        return nullptr;
      }
      return entry;
    }
  }

  // In general, this should be unreachable, though if file.paths() is empty,
  // I suppose it is possible.
  return nullptr;
}
}
}
