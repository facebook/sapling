/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#ifndef _WIN32

#include <array>
#include <cstddef>
#include <cstdint>
#include <memory>
#include <vector>

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
#ifdef __linux__
  struct RingPool;

  struct RingEntry {
    RingPool* pool{nullptr};
    size_t queueId{0};
    fuse_uring_req_header* requestHeader{nullptr};
    void* payload{nullptr};
    size_t payloadSize{0};
    uint64_t requestCommitId{0};
    std::array<iovec, 2> iov{};
  };

  struct RingQueue {
    RingPool* pool{nullptr};
    size_t queueId{0};
    int eventFd{-1};
    size_t requestHeaderSize{sizeof(fuse_uring_req_header)};
    std::vector<RingEntry> entries;
  };

  struct RingPool {
    size_t queueDepth{0};
    size_t maxRequestPayloadSize{0};
    std::vector<RingQueue> queues;
  };

  std::unique_ptr<RingPool> ringPool_;
#endif
  uint32_t queueDepth_;
};

} // namespace facebook::eden

#endif
