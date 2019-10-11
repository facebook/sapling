/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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
