/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/store/BackingStore.h"

#include "eden/fs/model/Blob.h"

namespace facebook {
namespace eden {

folly::Future<std::unique_ptr<Blob>> BackingStore::verifyEmptyBlob(
    const Hash&) {
  return folly::makeFuture(nullptr);
}

} // namespace eden
} // namespace facebook
