/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "FileHandle.h"

using namespace folly;

namespace facebook {
namespace eden {
namespace fusell {

bool FileHandle::usesDirectIO() const { return false; }
bool FileHandle::preserveCache() const { return false; }
bool FileHandle::isSeekable() const { return true; }
folly::Future<folly::Unit> FileHandle::release() { return Unit{}; }

folly::Future<struct flock> FileHandle::getlk(struct flock lock,
                                              uint64_t lock_owner) {
    FUSELL_NOT_IMPL();
}

folly::Future<folly::Unit> FileHandle::setlk(struct flock lock,
                                             bool sleep,
                                             uint64_t lock_owner) {
    FUSELL_NOT_IMPL();
}

}
}
}
