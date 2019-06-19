/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include <memory>

namespace folly {
class File;
}

namespace facebook {
namespace eden {

class PrivHelperServer;
class UserInfo;
class PrivHelper;

/**
 * Fork a separate privileged helper process, for performing mounts.
 *
 * This function should be very early on during program initialization, before
 * any other threads are forked.  After it is called UserInfo::dropPrivileges()
 * should be called to return the desired user privileges.
 */
std::unique_ptr<PrivHelper> startPrivHelper(const UserInfo& userInfo);

/**
 * Start a privhelper process using a custom PrivHelperServer class.
 *
 * This is really only intended for use in unit tests.
 */
std::unique_ptr<PrivHelper> startPrivHelper(
    PrivHelperServer* server,
    const UserInfo& userInfo);

/**
 * Create a PrivHelper client object using the specified connection rather than
 * forking a new privhelper server process.
 *
 * This is primarily intended for use in unit tests.
 */
std::unique_ptr<PrivHelper> createTestPrivHelper(folly::File&& conn);

} // namespace eden
} // namespace facebook
