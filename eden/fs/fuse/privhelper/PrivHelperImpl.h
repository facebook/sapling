/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/portability/GFlags.h>
#include <memory>
#include "eden/fs/utils/PathFuncs.h"

namespace folly {
class File;
}

namespace facebook::eden {

class PrivHelperServer;
class UserInfo;
class PrivHelper;

/**
 * Spawn a separate privileged helper process, for performing mounts.
 *
 * This function should be very early on during program initialization, before
 * any other threads are forked.  After it is called UserInfo::dropPrivileges()
 * should be called to return the desired user privileges.
 */
std::unique_ptr<PrivHelper>
startOrConnectToPrivHelper(const UserInfo& userInfo, int argc, char** argv);

#ifndef _WIN32
/**
 * Create a PrivHelper client object using the specified connection rather than
 * forking a new privhelper server process.
 *
 * This is primarily intended for use in unit tests.
 */
std::unique_ptr<PrivHelper> createTestPrivHelper(folly::File&& conn);

#endif // !_WIN32

} // namespace facebook::eden
