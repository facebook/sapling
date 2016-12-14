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

#include <folly/futures/Future.h>

#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/LocalStore.h"

using folly::Future;
using folly::makeFuture;
using std::unique_ptr;

namespace facebook {
namespace eden {

TestBackingStore::TestBackingStore(std::shared_ptr<LocalStore> localStore)
    : localStore_(std::move(localStore)) {}

TestBackingStore::~TestBackingStore() {}

Future<unique_ptr<Tree>> TestBackingStore::getTree(const Hash& /* id */) {
  return makeFuture<unique_ptr<Tree>>(
      std::runtime_error("getTree() not implemented yet"));
}

Future<unique_ptr<Blob>> TestBackingStore::getBlob(const Hash& /* id */) {
  return makeFuture<unique_ptr<Blob>>(
      std::runtime_error("getBlob() not implemented yet"));
}

Future<unique_ptr<Tree>> TestBackingStore::getTreeForCommit(
    const Hash& commitID) {
  return makeFuture(localStore_->getTree(commitID));
}
}
} // facebook::eden
