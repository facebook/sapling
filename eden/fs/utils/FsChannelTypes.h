/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include <folly/File.h>
#ifdef __linux__
#include "eden/fs/third-party/fuse_kernel_linux.h" // @manual
#elif __APPLE__
#include "eden/fs/third-party/fuse_kernel_osxfuse.h" // @manual
#endif

namespace facebook::eden {

#ifndef _WIN32
using FuseOpcode = decltype(std::declval<fuse_in_header>().opcode);
#endif
/** Encapsulates the fuse device & connection information for a mount point.
 * This is the data that is required to be passed to a new process when
 * performing a graceful restart in order to re-establish the FuseChannel.
 */
struct FuseChannelData {
  folly::File fd;
#ifndef _WIN32
  fuse_init_out connInfo;
#endif
};

struct NfsChannelData {
  folly::File nfsdSocketFd;
};

struct ProjFsChannelData {
  // TODO fill this in with data to support takeover on windows
};

} // namespace facebook::eden
