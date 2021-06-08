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

namespace facebook::eden {

ReCasBackingStore::ReCasBackingStore(
    std::shared_ptr<LocalStore> /** localStore **/) {}

ReCasBackingStore::~ReCasBackingStore() = default;

RootId ReCasBackingStore::parseRootId(folly::StringPiece rootId) {
  return RootId{rootId.str()};
}

std::string ReCasBackingStore::renderRootId(const RootId& rootId) {
  return rootId.value();
}

folly::SemiFuture<std::unique_ptr<Tree>> ReCasBackingStore::getRootTree(
    const RootId& /** id **/,
    ObjectFetchContext& /** context **/) {
  throw std::domain_error("unimplemented:");
}

folly::SemiFuture<std::unique_ptr<Tree>> ReCasBackingStore::getTree(
    const Hash& /** id **/,
    ObjectFetchContext& /** context **/) {
  throw std::domain_error("unimplemented");
}

folly::SemiFuture<std::unique_ptr<Blob>> ReCasBackingStore::getBlob(
    const Hash& /** id **/,
    ObjectFetchContext& /** context **/) {
  throw std::domain_error("unimplemented");
}

} // namespace facebook::eden
