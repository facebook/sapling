/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/testharness/FakeObjectStore.h"

#include <folly/MapUtil.h>
#include <folly/futures/Future.h>

#include "eden/common/utils/ImmediateFuture.h"

using folly::makeSemiFuture;
using folly::SemiFuture;
using std::make_shared;
using std::shared_ptr;

namespace facebook::eden {

FakeObjectStore::FakeObjectStore() = default;

FakeObjectStore::~FakeObjectStore() = default;

void FakeObjectStore::addTree(Tree&& tree) {
  auto treeId = tree.getObjectId();
  trees_.emplace(std::move(treeId), std::move(tree));
}

void FakeObjectStore::addBlob(ObjectId id, Blob&& blob) {
  blobs_.emplace(std::move(id), std::move(blob));
}

void FakeObjectStore::setTreeForCommit(const RootId& commitID, Tree&& tree) {
  auto ret = commits_.emplace(commitID, std::move(tree));
  if (!ret.second) {
    // Warn the caller that a Tree has already been specified for this commit,
    // which is likely a logical error. If this turns out to be something that
    // we want to do in a test, then we can change this behavior.
    throw std::runtime_error(
        fmt::format("tree already added for commit with id {}", commitID));
  }
}

ImmediateFuture<IObjectStore::GetRootTreeResult> FakeObjectStore::getRootTree(
    const RootId& commitID,
    const ObjectFetchContextPtr&) const {
  ++commitAccessCounts_[commitID];
  auto iter = commits_.find(commitID);
  if (iter == commits_.end()) {
    return makeSemiFuture<GetRootTreeResult>(std::domain_error(
        fmt::format("tree data for commit {} not found", commitID)));
  }
  auto tree = make_shared<const Tree>(iter->second);
  return GetRootTreeResult{tree, tree->getObjectId()};
}

ImmediateFuture<std::shared_ptr<const Tree>> FakeObjectStore::getTree(
    const ObjectId& id,
    const ObjectFetchContextPtr&) const {
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
    const ObjectFetchContextPtr&) const {
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
    const ObjectFetchContextPtr&) const {
  return folly::unit;
}

size_t FakeObjectStore::getAccessCount(const ObjectId& id) const {
  if (auto* item = folly::get_ptr(accessCounts_, id)) {
    return *item;
  } else {
    return 0;
  }
}

} // namespace facebook::eden
