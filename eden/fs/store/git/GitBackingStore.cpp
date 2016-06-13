/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "GitBackingStore.h"

#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/LocalStore.h"

using folly::StringPiece;
using std::unique_ptr;

namespace facebook {
namespace eden {

GitBackingStore::GitBackingStore(StringPiece repository, LocalStore* localStore)
    : localStore_{localStore} {}

GitBackingStore::~GitBackingStore() {}

unique_ptr<Tree> GitBackingStore::getTree(const Hash& id) {
  // TODO: GitBackingStore currently requires that all data be pre-imported
  return nullptr;
}

unique_ptr<Blob> GitBackingStore::getBlob(const Hash& id) {
  // TODO: GitBackingStore currently requires that all data be pre-imported
  return nullptr;
}

unique_ptr<Tree> GitBackingStore::getTreeForCommit(const Hash& commitID) {
  // TODO: At the moment stores a tree ID in the SNAPSHOT, and not a commit ID.
  // Just look up in the tree in the LocalStore, using the specified hash.
  return localStore_->getTree(commitID);
}
}
}
