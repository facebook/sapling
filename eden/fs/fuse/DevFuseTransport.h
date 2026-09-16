/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#ifndef _WIN32

#include <folly/Function.h>

#include "eden/fs/fuse/FuseTransport.h"

namespace facebook::eden {

class DevFuseTransport final : public FuseTransport {
 public:
  const char* getName() const override;
  size_t getWorkerThreadCount(size_t defaultThreadCount) const override;
  ssize_t readInitPacket(int fd, void* buf, size_t size) const override;
  void processSession(FuseChannel& channel) override;
  // stopFd is a nonblocking eventfd, or -1 for the classic blocking reader.
  // onReady runs after configuring the reader and is not retained.
  void processSession(
      FuseChannel& channel,
      int stopFd,
      folly::FunctionRef<void()> onReady);
  void replyError(
      FuseChannel& channel,
      const fuse_in_header& request,
      int errorCode) const override;
  void sendRawReply(FuseChannel& channel, const iovec iov[], size_t count)
      const override;
};

} // namespace facebook::eden

#endif
