/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "FakeObjectStore.h"

#include <folly/MapUtil.h>
#include <folly/String.h>
#include <folly/futures/Future.h>

#include "eden/fs/utils/ImmediateFuture.h"

using folly::makeSemiFuture;
using folly::SemiFuture;
using std::make_shared;
using std::shared_ptr;

namespace facebook::eden {

FakeObjectStore::FakeObjectStore() = default;

FakeObjectStore::~FakeObjectStore() = default;

void FakeObjectStore::addTree(Tree&& tree) {
  auto treeHash = tree.getHash();
  trees_.emplace(std::move(treeHash), std::move(tree));
}

void FakeObjectStore::addBlob(Blob&& blob) {
  auto blobHash = blob.getHash();
  blobs_.emplace(blobHash, std::move(blob));
}

void FakeObjectStore::setTreeForCommit(const RootId& commitID, Tree&& tree) {
  auto ret = commits_.emplace(commitID, std::move(tree));
  if (!ret.second) {
    // Warn the caller that a Tree has already been specified for this commit,
    // which is likely a logical error. If this turns out to be something that
    // we want to do in a test, then we can change this behavior.
    throw std::runtime_error(folly::to<std::string>(
        "tree already added for commit with id ", commitID));
  }
}

ImmediateFuture<shared_ptr<const Tree>> FakeObjectStore::getRootTree(
    const RootId& commitID,
    ObjectFetchContext&) const {
  ++commitAccessCounts_[commitID];
  auto iter = commits_.find(commitID);
  if (iter == commits_.end()) {
    return makeSemiFuture<shared_ptr<const Tree>>(
        std::domain_error(folly::to<std::string>(
            "tree data for commit ", commitID, " not found")));
  }
  return make_shared<const Tree>(iter->second);
}

ImmediateFuture<std::shared_ptr<const Tree>> FakeObjectStore::getTree(
    const ObjectId& id,
    ObjectFetchContext&) const {
  ++accessCounts_[id];
  auto iter = trees_.find(id);
  if (iter == trees_.end()) {
    return makeImmediateFuture<std::shared_ptr<const Tree>>(
        std::domain_error(fmt::format("tree {} not found", id)));
  }
  return make_shared<const Tree>(iter->second);
}

ImmediateFuture<std::shared_ptr<const Blob>> FakeObjectStore::getBlob(
    const ObjectId& id,
    ObjectFetchContext&) const {
  ++accessCounts_[id];
  auto iter = blobs_.find(id);
  if (iter == blobs_.end()) {
    return makeImmediateFuture<shared_ptr<const Blob>>(
        std::domain_error(fmt::format("blob {} not found", id)));
  }
  return make_shared<const Blob>(iter->second);
}

ImmediateFuture<folly::Unit> FakeObjectStore::prefetchBlobs(
    ObjectIdRange,
    ObjectFetchContext&) const {
  return folly::unit;
}

size_t FakeObjectStore::getAccessCount(const ObjectId& hash) const {
  if (auto* item = folly::get_ptr(accessCounts_, hash)) {
    return *item;
  } else {
    return 0;
  }
}

} // namespace facebook::eden
