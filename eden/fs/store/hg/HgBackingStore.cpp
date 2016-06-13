/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "HgBackingStore.h"

#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/LocalStore.h"

using folly::StringPiece;
using std::unique_ptr;

namespace facebook {
namespace eden {

HgBackingStore::HgBackingStore(StringPiece repository, LocalStore* localStore)
    : importer_(folly::construct_in_place, repository, localStore),
      localStore_(localStore) {}

HgBackingStore::~HgBackingStore() {}

unique_ptr<Tree> HgBackingStore::getTree(const Hash& id) {
  // HgBackingStore imports all relevant Tree objects inside getTreeForCommit()
  // We should never have a case where we are asked for a Tree that hasn't
  // already been imported.
  LOG(ERROR) << "HgBackingStore asked for unknown tree ID " << id.toString();
  return nullptr;
}

unique_ptr<Blob> HgBackingStore::getBlob(const Hash& id) {
  // TODO
  return nullptr;
}

unique_ptr<Tree> HgBackingStore::getTreeForCommit(const Hash& commitID) {
  // TODO: Store a mapping of commitID --> treeID, and
  // check to see if we have already imported this commit.
  auto treeHash = importer_->importManifest(commitID.toString());
  VLOG(1) << "imported mercurial commit " << commitID.toString() << " as tree "
          << treeHash.toString();
  return localStore_->getTree(treeHash);
}
}
} // facebook::eden
