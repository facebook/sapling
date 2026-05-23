/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#ifndef _WIN32

#include <cstdint>

#include "eden/fs/fuse/FuseTransport.h"

namespace facebook::eden {

class IoUringFuseTransport final : public FuseTransport {
 public:
  explicit IoUringFuseTransport(uint32_t queueDepth);

  const char* getName() const override;
  ssize_t readInitPacket(int fd, void* buf, size_t size) const override;
  void processSession(FuseChannel& channel) override;
  void replyError(
      FuseChannel& channel,
      const fuse_in_header& request,
      int errorCode) const override;
  void sendRawReply(FuseChannel& channel, const iovec iov[], size_t count)
      const override;

 private:
  uint32_t queueDepth_;
};

} // namespace facebook::eden

#endif
