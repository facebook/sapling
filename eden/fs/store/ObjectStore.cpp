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
    VLOG(4) << "tree " << id << " found in local store";
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
    VLOG(2) << "unable to find tree " << id;
    throw std::domain_error(
        folly::to<string>("tree ", id.toString(), " not found"));
  }

  // TODO: For now, the BackingStore objects actually end up already
  // saving the Tree object in the LocalStore, so we don't do anything here.
  //
  // localStore_->putTree(tree.get());
  VLOG(3) << "tree " << id << " retrieved from backing store";
  return tree;
}

unique_ptr<Blob> ObjectStore::getBlob(const Hash& id) const {
  auto blob = localStore_->getBlob(id);
  if (blob) {
    VLOG(4) << "blob " << id << "  found in local store";
    return blob;
  }

  // Look in the BackingStore
  blob = backingStore_->getBlob(id);
  if (!blob) {
    VLOG(2) << "unable to find blob " << id;
    // TODO: Perhaps we should do some short-term negative caching?
    throw std::domain_error(
        folly::to<string>("blob ", id.toString(), " not found"));
  }

  VLOG(3) << "blob " << id << "  retrieved from backing store";
  localStore_->putBlob(id, blob.get());
  return blob;
}

unique_ptr<Tree> ObjectStore::getTreeForCommit(const Hash& commitID) const {
  VLOG(3) << "getTreeForCommit(" << commitID << ")";

  // For now we assume that the BackingStore will insert the Tree into the
  // LocalStore on its own.
  auto tree = backingStore_->getTreeForCommit(commitID);
  if (!tree) {
    throw std::domain_error(
        folly::to<string>("unable to import commit ", commitID.toString()));
  }
  return tree;
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

  auto metadata = localStore_->putBlob(id, blob.get());
  return std::make_unique<Hash>(metadata.sha1);
}
}
} // facebook::eden
