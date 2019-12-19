/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/EmptyBackingStore.h"

#include <folly/futures/Future.h>
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"

using folly::Future;
using folly::makeFuture;
using folly::makeSemiFuture;
using folly::SemiFuture;
using std::unique_ptr;

namespace facebook {
namespace eden {

EmptyBackingStore::EmptyBackingStore() {}

EmptyBackingStore::~EmptyBackingStore() {}

Future<unique_ptr<Tree>> EmptyBackingStore::getTree(const Hash& /* id */) {
  return makeFuture<unique_ptr<Tree>>(std::domain_error("empty backing store"));
}

SemiFuture<unique_ptr<Blob>> EmptyBackingStore::getBlob(const Hash& /* id */) {
  return makeSemiFuture<unique_ptr<Blob>>(
      std::domain_error("empty backing store"));
}

Future<unique_ptr<Tree>> EmptyBackingStore::getTreeForCommit(
    const Hash& /* commitID */) {
  return makeFuture<unique_ptr<Tree>>(std::domain_error("empty backing store"));
}

SemiFuture<std::unique_ptr<Tree>> EmptyBackingStore::getTreeForManifest(
    const Hash& /* commitID */,
    const Hash& /* manifestID */) {
  return makeFuture<unique_ptr<Tree>>(std::domain_error("empty backing store"));
}
} // namespace eden
} // namespace facebook
