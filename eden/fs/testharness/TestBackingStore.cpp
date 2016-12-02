/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "TestBackingStore.h"

#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/LocalStore.h"

using std::unique_ptr;

namespace facebook {
namespace eden {

TestBackingStore::TestBackingStore(std::shared_ptr<LocalStore> localStore)
    : localStore_(std::move(localStore)) {}

TestBackingStore::~TestBackingStore() {}

unique_ptr<Tree> TestBackingStore::getTree(const Hash& /* id */) {
  return nullptr;
}

unique_ptr<Blob> TestBackingStore::getBlob(const Hash& /* id */) {
  return nullptr;
}

unique_ptr<Tree> TestBackingStore::getTreeForCommit(const Hash& commitID) {
  return localStore_->getTree(commitID);
}
}
} // facebook::eden
