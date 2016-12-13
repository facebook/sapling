/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "FakeObjectStore.h"
#include <folly/String.h>

using std::make_unique;
using std::unique_ptr;
using std::unordered_map;

namespace facebook {
namespace eden {

FakeObjectStore::FakeObjectStore()
    : trees_(unordered_map<Hash, Tree>()),
      blobs_(unordered_map<Hash, Blob>()),
      commits_(unordered_map<Hash, Tree>()),
      sha1s_(unordered_map<Hash, Hash>()) {}

FakeObjectStore::~FakeObjectStore() {}

void FakeObjectStore::addTree(Tree&& tree) {
  trees_.emplace(tree.getHash(), std::move(tree));
}

void FakeObjectStore::addBlob(Blob&& blob) {
  blobs_.emplace(blob.getHash(), std::move(blob));
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

void FakeObjectStore::setSha1ForBlob(const Blob& blob, const Hash& sha1) {
  sha1s_[blob.getHash()] = sha1;
}

unique_ptr<Tree> FakeObjectStore::getTree(const Hash& id) const {
  auto iter = trees_.find(id);
  if (iter != trees_.end()) {
    return make_unique<Tree>(iter->second);
  } else {
    throw std::domain_error("tree " + id.toString() + " not found");
  }
}

unique_ptr<Blob> FakeObjectStore::getBlob(const Hash& id) const {
  auto iter = blobs_.find(id);
  if (iter != blobs_.end()) {
    return make_unique<Blob>(iter->second);
  } else {
    throw std::domain_error("blob " + id.toString() + " not found");
  }
}

unique_ptr<Tree> FakeObjectStore::getTreeForCommit(const Hash& id) const {
  auto iter = commits_.find(id);
  if (iter != commits_.end()) {
    return make_unique<Tree>(iter->second);
  } else {
    throw std::domain_error("commit " + id.toString() + " not found");
  }
}

unique_ptr<Hash> FakeObjectStore::getSha1ForBlob(const Hash& id) const {
  auto iter = sha1s_.find(id);
  if (iter != sha1s_.end()) {
    return make_unique<Hash>(iter->second);
  } else {
    throw std::domain_error("blob " + id.toString() + " not found");
  }
}
}
}
