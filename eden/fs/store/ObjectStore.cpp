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
#include "LocalStore.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Tree.h"

using folly::IOBuf;
using std::shared_ptr;
using std::string;
using std::unique_ptr;

namespace facebook {
namespace eden {

ObjectStore::ObjectStore(shared_ptr<LocalStore> localStore)
    : localStore_(std::move(localStore)) {}

ObjectStore::~ObjectStore() {}

/*
 * TODO: Support querying from a BackingStore object if the data is not found
 * in the LocalStore.
 */

unique_ptr<Tree> ObjectStore::getTree(const Hash& id) const {
  auto tree = localStore_->getTree(id);
  if (!tree) {
    throw std::domain_error(
        folly::to<string>("tree ", id.toString(), " not found"));
  }
  return tree;
}

unique_ptr<Blob> ObjectStore::getBlob(const Hash& id) const {
  auto blob = localStore_->getBlob(id);
  if (!blob) {
    throw std::domain_error(
        folly::to<string>("blob ", id.toString(), " not found"));
  }
  return blob;
}

unique_ptr<Hash> ObjectStore::getSha1ForBlob(const Hash& id) const {
  auto sha1 = localStore_->getSha1ForBlob(id);
  if (!sha1) {
    throw std::domain_error(
        folly::to<string>("SHA-1 for blob ", id.toString(), " not found"));
  }
  return sha1;
}
}
} // facebook::eden
