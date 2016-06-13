/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "ObjectStore.h"

#include <folly/Conv.h>
#include <folly/io/IOBuf.h>
#include <stdexcept>
#include "BackingStore.h"
#include "LocalStore.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Tree.h"

using folly::IOBuf;
using std::shared_ptr;
using std::string;
using std::unique_ptr;

namespace facebook {
namespace eden {

ObjectStore::ObjectStore(
    shared_ptr<LocalStore> localStore,
    shared_ptr<BackingStore> backingStore)
    : localStore_(std::move(localStore)),
      backingStore_(std::move(backingStore)) {}

ObjectStore::~ObjectStore() {}

unique_ptr<Tree> ObjectStore::getTree(const Hash& id) const {
  // Check in the LocalStore first
  auto tree = localStore_->getTree(id);
  if (tree) {
    return tree;
  }

  // TODO: We probably should build a mechanism to check if the same ID is
  // being looked up simultanously in separate threads, and share the work.
  // This may require more thinking about the memory model, since currently
  // getTree() returns a unique_ptr, which cannot be shared.  We should figure
  // out if we manage simultaneous lookups in the ObjectStore, or higher up
  // in the inode code.

  // Look in the BackingStore
  tree = backingStore_->getTree(id);
  if (!tree) {
    // TODO: Perhaps we should do some short-term negative caching?
    throw std::domain_error(
        folly::to<string>("tree ", id.toString(), " not found"));
  }

  // TODO:
  // localStore_->putTree(tree.get());
  return tree;
}

unique_ptr<Blob> ObjectStore::getBlob(const Hash& id) const {
  auto blob = localStore_->getBlob(id);
  if (blob) {
    return blob;
  }

  // Look in the BackingStore
  blob = backingStore_->getBlob(id);
  if (!blob) {
    // TODO: Perhaps we should do some short-term negative caching?
    throw std::domain_error(
        folly::to<string>("blob ", id.toString(), " not found"));
  }

  // TODO:
  // localStore_->putBlob(blob.get());
  return blob;
}

unique_ptr<Hash> ObjectStore::getSha1ForBlob(const Hash& id) const {
  auto sha1 = localStore_->getSha1ForBlob(id);
  if (sha1) {
    return sha1;
  }

  // TODO: We should probably build a smarter API so we can ask the
  // BackingStore for just the SHA1 if it can compute it more efficiently
  // without having to get the full blob data.

  // Look in the BackingStore
  auto blob = backingStore_->getBlob(id);
  if (!blob) {
    // TODO: Perhaps we should do some short-term negative caching?
    throw std::domain_error(
        folly::to<string>("blob ", id.toString(), " not found"));
  }

  // TODO:
  // localStore_->putBlob(blob.get());
  auto sha1Obj = Hash::sha1(&blob->getContents());
  return std::make_unique<Hash>(sha1Obj);
}
}
} // facebook::eden
