/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include <folly/File.h>
#ifdef __linux__
#include "eden/fs/third-party/fuse_kernel_linux.h"
#elif defined(__APPLE__)
#include "eden/fs/third-party/fuse_kernel_osxfuse.h"
#else
#error need a fuse kernel header to be included for your OS!
#endif

namespace facebook::eden {

using FuseOpcode = decltype(std::declval<fuse_in_header>().opcode);

/** Encapsulates the fuse device & connection information for a mount point.
 * This is the data that is required to be passed to a new process when
 * performing a graceful restart in order to re-establish the FuseChannel.
 */
struct FuseChannelData {
  folly::File fd;
  fuse_init_out connInfo;
};

} // namespace facebook::eden
