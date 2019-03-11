/*
 *  Copyright (c) 2019-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include "eden/fs/model/Tree.h"

namespace facebook {
namespace eden {
std::unique_ptr<Tree> parseMononokeTree(
    std::unique_ptr<folly::IOBuf>&& buf,
    const Hash& id);
} // namespace eden
} // namespace facebook
