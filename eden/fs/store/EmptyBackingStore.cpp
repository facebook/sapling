/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "EmptyBackingStore.h"

#include <folly/futures/Future.h>
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"

using folly::Future;
using folly::makeFuture;
using std::unique_ptr;

namespace facebook {
namespace eden {

EmptyBackingStore::EmptyBackingStore() {}

EmptyBackingStore::~EmptyBackingStore() {}

Future<unique_ptr<Tree>> EmptyBackingStore::getTree(const Hash& /* id */) {
  return makeFuture<unique_ptr<Tree>>(std::domain_error("empty backing store"));
}

Future<unique_ptr<Blob>> EmptyBackingStore::getBlob(const Hash& /* id */) {
  return makeFuture<unique_ptr<Blob>>(std::domain_error("empty backing store"));
}

Future<unique_ptr<Tree>> EmptyBackingStore::getTreeForCommit(
    const Hash& /* commitID */) {
  return makeFuture<unique_ptr<Tree>>(std::domain_error("empty backing store"));
}
}
} // facebook::eden
