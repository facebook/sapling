/*
 *  Copyright (c) 2017-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once
#include <folly/File.h>
#ifdef __linux__
#include "eden/third-party/fuse_kernel_linux.h" // @manual=//eden/third-party:fuse_kernel
#elif defined(__APPLE__)
#include "external/osxfuse/kext/osxfuse/fuse_kernel.h" // @manual
#else
#error need a fuse kernel header to be included for your OS!
#endif
namespace facebook {
namespace eden {

using FuseOpcode = decltype(std::declval<fuse_in_header>().opcode);

/** Encapsulates the fuse device & connection information for a mount point.
 * This is the data that is required to be passed to a new process when
 * performing a graceful restart in order to re-establish the FuseChannel.
 */
struct FuseChannelData {
  folly::File fd;
  fuse_init_out connInfo;
};

} // namespace eden
} // namespace facebook
