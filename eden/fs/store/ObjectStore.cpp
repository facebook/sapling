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

#include <folly/io/IOBuf.h>
#include "LocalStore.h"

using folly::IOBuf;
using std::shared_ptr;
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
  return localStore_->getTree(id);
}

unique_ptr<Blob> ObjectStore::getBlob(const Hash& id) const {
  return localStore_->getBlob(id);
}

unique_ptr<Hash> ObjectStore::getSha1ForBlob(const Hash& id) const {
  return localStore_->getSha1ForBlob(id);
}
}
} // facebook::eden
