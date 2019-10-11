/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <err.h>
#include <sysexits.h>
#include <unistd.h>
#include <array>

#include <folly/init/Init.h>
#include <folly/logging/Init.h>
#include <folly/logging/xlog.h>

#include "eden/fs/fuse/privhelper/UserInfo.h"

using namespace facebook::eden;

FOLLY_INIT_LOGGING_CONFIG("eden=INFO");

/*
 * This is a samll helper program for manually testing the
 * UserInfo::dropPrivileges() functionality.
 *
 * If run as a setuid binary or under sudo it prints out the desired user
 * privileges, then drops privileges and runs the specified command.
 * If no command was given, /bin/sh is run.
 */
int main(int argc, char** argv) {
  folly::init(&argc, &argv);

  auto info = UserInfo::lookup();
  printf("Username: %s\n", info.getUsername().c_str());
  printf("UID/GID:  %d/%d\n", info.getUid(), info.getGid());
  printf("Home Dir: %s\n", info.getHomeDirectory().value().c_str());

  if (geteuid() != 0) {
    fprintf(
        stderr, "error: unable to drop privileges unless running as root\n");
    return EX_USAGE;
  }

  info.dropPrivileges();

  if (argc < 2) {
    // Run a shell
    printf("Successfully dropped privileges.  Running /bin/sh\n");
    execl("/bin/sh", "sh", nullptr);
  } else {
    // Run the command specified in the remaining arguments.
    // Users can use the "--" argument to prevent gflags from processing
    // any remaining arguments in the command in case they start with "-"
    printf("Successfully dropped privileges.  Running %s\n", argv[1]);
    execvp(argv[1], argv + 1);
  }

  err(EX_OSERR, "exec failed");
}
