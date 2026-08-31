/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <gflags/gflags.h>
#include <memory>
#include "eden/common/utils/PathFuncs.h"

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
 * Absolute path of the killswitch file whose presence disables disclaiming
 * macOS TCC responsibility when spawning the privhelper.
 *
 * The path is deliberately hardcoded rather than derived from --etcEdenDir:
 * the privhelper is spawned before any command-line or config parsing, so
 * this must be resolvable with no parsed state. An existence check never
 * reads file content, which keeps it safe to perform while the process may
 * still hold elevated privileges (sudo/dev flow). A plain file under
 * /etc/eden is oncall-operable (`sudo touch`) and Chef-manageable.
 *
 * NOTE: edenfsctl checks the same killswitch file. This path must stay in
 * sync with KILLSWITCH_PATH in eden/fs/cli_rs/edenfsctl/src/tcc_disclaim.rs.
 */
constexpr const char* kTccDisclaimKillswitchPath =
    "/etc/eden/disable-tcc-disclaim";

/**
 * Whether the TCC-disclaim killswitch file exists. The path parameter exists
 * for unit tests; production callers use the default.
 */
bool tccDisclaimKillswitchPresent(
    const char* path = kTccDisclaimKillswitchPath);

/**
 * Create a PrivHelper client object using the specified connection rather than
 * forking a new privhelper server process.
 *
 * This is primarily intended for use in unit tests.
 */
std::unique_ptr<PrivHelper> createTestPrivHelper(folly::File conn);

#endif // !_WIN32

} // namespace facebook::eden
