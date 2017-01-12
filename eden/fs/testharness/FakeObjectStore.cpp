/*
 *  Copyright (c) 2017, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "FakeObjectStore.h"

#include <folly/Optional.h>
#include <folly/String.h>
#include <folly/futures/Future.h>

using folly::Future;
using folly::makeFuture;
using std::make_unique;
using std::unique_ptr;
using std::unordered_map;

namespace facebook {
namespace eden {

FakeObjectStore::FakeObjectStore() {}

FakeObjectStore::~FakeObjectStore() {}

void FakeObjectStore::addTree(Tree&& tree) {
  auto treeHash = tree.getHash();
  trees_.emplace(std::move(treeHash), std::move(tree));
}

void FakeObjectStore::addBlob(Blob&& blob) {
  // Compute the blob metadata
  auto sha1 = Hash::sha1(&blob.getContents());
  auto metadata =
      BlobMetadata{sha1, blob.getContents().computeChainDataLength()};

  auto blobHash = blob.getHash();
  blobs_.emplace(blobHash, std::move(blob));
  blobMetadata_.emplace(std::move(blobHash), metadata);
}

void FakeObjectStore::setTreeForCommit(const Hash& commitID, Tree&& tree) {
  auto ret = commits_.emplace(commitID, std::move(tree));
  if (!ret.second) {
    // Warn the caller that a Tree has already been specified for this commit,
    // which is likely a logical error. If this turns out to be something that
    // we want to do in a test, then we can change this behavior.
    throw std::runtime_error(folly::to<std::string>(
        "tree already added for commit with id ", commitID));
  }
}

unique_ptr<Tree> FakeObjectStore::getTree(const Hash& id) const {
  return getTreeFuture(id).get();
}

Future<std::unique_ptr<Tree>> FakeObjectStore::getTreeFuture(
    const Hash& id) const {
  auto iter = trees_.find(id);
  if (iter == trees_.end()) {
    return makeFuture<unique_ptr<Tree>>(
        std::domain_error("tree " + id.toString() + " not found"));
  }
  return makeFuture(make_unique<Tree>(iter->second));
}

unique_ptr<Blob> FakeObjectStore::getBlob(const Hash& id) const {
  return getBlobFuture(id).get();
}

Future<std::unique_ptr<Blob>> FakeObjectStore::getBlobFuture(
    const Hash& id) const {
  auto iter = blobs_.find(id);
  if (iter == blobs_.end()) {
    return makeFuture<unique_ptr<Blob>>(
        std::domain_error("blob " + id.toString() + " not found"));
  }
  return makeFuture(make_unique<Blob>(iter->second));
}

Future<unique_ptr<Tree>> FakeObjectStore::getTreeForCommit(
    const Hash& commitID) const {
  auto iter = commits_.find(commitID);
  if (iter == commits_.end()) {
    return makeFuture<unique_ptr<Tree>>(std::domain_error(
        "tree data for commit " + commitID.toString() + " not found"));
  }
  return makeFuture(make_unique<Tree>(iter->second));
}

Hash FakeObjectStore::getSha1ForBlob(const Hash& id) const {
  return getBlobMetadata(id).get().sha1;
}

Future<BlobMetadata> FakeObjectStore::getBlobMetadata(const Hash& id) const {
  auto iter = blobMetadata_.find(id);
  if (iter == blobMetadata_.end()) {
    return makeFuture<BlobMetadata>(
        std::domain_error("metadata for blob " + id.toString() + " not found"));
  }
  return makeFuture(iter->second);
}
}
}
