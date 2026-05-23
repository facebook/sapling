/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#ifndef _WIN32

#include <sys/types.h>
#include <sys/uio.h>
#include <cstddef>

#include "eden/fs/third-party/fuse_kernel_linux.h"

namespace facebook::eden {

class FuseChannel;

class FuseTransport {
 public:
  virtual ~FuseTransport() = default;

  virtual const char* getName() const = 0;
  virtual size_t getWorkerThreadCount(size_t defaultThreadCount) const {
    return defaultThreadCount;
  }
  virtual void requestStopWakeup() {}
  virtual ssize_t readInitPacket(int fd, void* buf, size_t size) const = 0;
  virtual void processSession(FuseChannel& channel) = 0;
  virtual void replyError(
      FuseChannel& channel,
      const fuse_in_header& request,
      int errorCode) const = 0;

  // The iovec array and all iov_base pointers are borrowed and may point to
  // caller-owned stack storage. Implementations MUST either fully consume the
  // reply before returning or copy the data into transport-owned storage before
  // any asynchronous use. Implementations MUST NOT retain these pointers after
  // this call returns.
  virtual void
  sendRawReply(FuseChannel& channel, const iovec iov[], size_t count) const = 0;
};

} // namespace facebook::eden

#endif
