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
#include "eden/fs/utils/UnboundedQueueThreadPool.h"

namespace facebook {
namespace eden {

ServerState::ServerState(
    UserInfo userInfo,
    std::shared_ptr<PrivHelper> privHelper,
    std::shared_ptr<UnboundedQueueThreadPool> threadPool)
    : userInfo_{std::move(userInfo)},
      privHelper_{std::move(privHelper)},
      threadPool_{std::move(threadPool)} {}

ServerState::~ServerState() {}

} // namespace eden
} // namespace facebook
