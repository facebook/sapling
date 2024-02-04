/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/EmptyBackingStore.h"

#include <folly/futures/Future.h>
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/ObjectFetchContext.h"

using folly::makeSemiFuture;
using folly::SemiFuture;

namespace facebook::eden {

EmptyBackingStore::EmptyBackingStore() = default;

EmptyBackingStore::~EmptyBackingStore() = default;

RootId EmptyBackingStore::parseRootId(folly::StringPiece /*rootId*/) {
  throw std::domain_error("empty backing store");
}

std::string EmptyBackingStore::renderRootId(const RootId& /*rootId*/) {
  throw std::domain_error("empty backing store");
}

ObjectId EmptyBackingStore::parseObjectId(folly::StringPiece /*objectId*/) {
  throw std::domain_error("empty backing store");
}

std::string EmptyBackingStore::renderObjectId(const ObjectId& /*objectId*/) {
  throw std::domain_error("empty backing store");
}

ImmediateFuture<BackingStore::GetRootTreeResult> EmptyBackingStore::getRootTree(
    const RootId& /* rootId */,
    const ObjectFetchContextPtr& /* context */) {
  return makeSemiFuture<GetRootTreeResult>(
      std::domain_error("empty backing store"));
}

SemiFuture<BackingStore::GetTreeResult> EmptyBackingStore::getTree(
    const ObjectId& /* id */,
    const ObjectFetchContextPtr& /* context */) {
  return makeSemiFuture<GetTreeResult>(
      std::domain_error("empty backing store"));
}

SemiFuture<BackingStore::GetBlobResult> EmptyBackingStore::getBlob(
    const ObjectId& /* id */,
    const ObjectFetchContextPtr& /* context */) {
  return makeSemiFuture<GetBlobResult>(
      std::domain_error("empty backing store"));
}

SemiFuture<BackingStore::GetBlobMetaResult> EmptyBackingStore::getBlobMetadata(
    const ObjectId& /* id */,
    const ObjectFetchContextPtr& /* context */) {
  return makeSemiFuture<GetBlobMetaResult>(
      std::domain_error("empty backing store"));
}

} // namespace facebook::eden
