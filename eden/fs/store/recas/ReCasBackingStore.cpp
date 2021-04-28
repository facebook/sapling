/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/recas/ReCasBackingStore.h"

#include <folly/futures/Future.h>
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/LocalStore.h"

namespace facebook {
namespace eden {

ReCasBackingStore::ReCasBackingStore(
    std::shared_ptr<LocalStore> /** localStore **/) {
  return;
};
ReCasBackingStore::~ReCasBackingStore() {
  return;
};

folly::SemiFuture<std::unique_ptr<Tree>> ReCasBackingStore::getTree(
    const Hash& /** id **/,
    ObjectFetchContext& /** context **/) {
  throw std::domain_error("unimplemented:");
};
folly::SemiFuture<std::unique_ptr<Blob>> ReCasBackingStore::getBlob(
    const Hash& /** id **/,
    ObjectFetchContext& /** context **/) {
  throw std::domain_error("unimplemented");
};

folly::SemiFuture<std::unique_ptr<Tree>> ReCasBackingStore::getTreeForCommit(
    const Hash& /** id **/,
    ObjectFetchContext& /** context **/) {
  throw std::domain_error("unimplemented");
};
folly::SemiFuture<std::unique_ptr<Tree>> ReCasBackingStore::getTreeForManifest(
    const Hash& /** commitID **/,
    const Hash& /** manifestID **/,
    ObjectFetchContext& /** context **/) {
  throw std::domain_error("unimplemented");
};

} // namespace eden
} // namespace facebook
