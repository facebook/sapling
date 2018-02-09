/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/inodes/ServerState.h"

#include "eden/fs/fuse/privhelper/PrivHelper.h"

namespace facebook {
namespace eden {

ServerState::ServerState() : userInfo_{UserInfo::lookup()} {}

ServerState::ServerState(
    UserInfo userInfo,
    std::unique_ptr<PrivHelper> privHelper)
    : userInfo_{std::move(userInfo)}, privHelper_{std::move(privHelper)} {}

ServerState::~ServerState() {}

} // namespace eden
} // namespace facebook
