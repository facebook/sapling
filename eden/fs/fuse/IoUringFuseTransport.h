/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#ifndef _WIN32

#include <array>
#include <atomic>
#include <cstddef>
#include <cstdint>
#include <cstdlib>
#include <memory>
#include <optional>
#include <unordered_map>
#include <vector>

#include <folly/Synchronized.h>
#include <folly/synchronization/CallOnce.h>

#ifdef __linux__
#include <liburing.h>
#endif

#include "eden/fs/fuse/FuseTransport.h"

namespace facebook::eden {

class IoUringFuseTransport final : public FuseTransport {
 public:
  explicit IoUringFuseTransport(uint32_t queueDepth);
  ~IoUringFuseTransport() override;
  IoUringFuseTransport(const IoUringFuseTransport&) = delete;
  IoUringFuseTransport& operator=(const IoUringFuseTransport&) = delete;
  IoUringFuseTransport(IoUringFuseTransport&&) = delete;
  IoUringFuseTransport& operator=(IoUringFuseTransport&&) = delete;

  const char* getName() const override;
  size_t getWorkerThreadCount(size_t defaultThreadCount) const override;
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
    using Buffer = std::unique_ptr<void, decltype(&free)>;

    RingPool* pool{nullptr};
    size_t queueId{0};
    Buffer requestHeaderStorage{nullptr, &free};
    Buffer payloadStorage{nullptr, &free};
    fuse_uring_req_header* requestHeader{nullptr};
    void* payload{nullptr};
    size_t payloadSize{0};
    uint64_t requestCommitId{0};
    std::array<iovec, 2> iov{};
  };

  struct RingQueue {
    RingQueue();
    ~RingQueue() noexcept;
    RingQueue(const RingQueue&) = delete;
    RingQueue& operator=(const RingQueue&) = delete;
    RingQueue(RingQueue&& other) noexcept;
    RingQueue& operator=(RingQueue&& other) noexcept;

    void reset() noexcept;
    void resetMovedFrom() noexcept;

    RingPool* pool{nullptr};
    size_t queueId{0};
    int eventFd{-1};
    size_t requestHeaderSize{sizeof(fuse_uring_req_header)};
    io_uring ring{};
    bool ringInitialized{false};
    std::vector<RingEntry> entries;
  };

  struct RingPool {
    RingPool() = default;
    RingPool(const RingPool&) = delete;
    RingPool& operator=(const RingPool&) = delete;
    RingPool(RingPool&&) = delete;
    RingPool& operator=(RingPool&&) = delete;
    ~RingPool() = default;

    size_t queueDepth{0};
    size_t maxRequestPayloadSize{0};
    std::vector<RingQueue> queues;
  };

  struct DecodedRequest {
    RingEntry* entry{nullptr};
    fuse_in_header header{};
    std::vector<uint8_t> arguments;
  };

  struct CqeResult {
    enum class Action {
      DispatchRequest,
      Ignored,
      StopRequested,
    };

    Action action{Action::Ignored};
    std::optional<DecodedRequest> request;
  };

  std::unique_ptr<RingPool> ringPool_;

  static size_t getConfiguredQueueCount(size_t defaultThreadCount);
  void initializeRingPool(size_t queueCount, size_t maxRequestPayloadSize);
  void initializeSession(FuseChannel& channel);
  void initializeQueue(RingQueue& queue, int fuseFd) const;
  void initializeEntryBuffers(RingQueue& queue, RingEntry& entry) const;
  void prepareFetchRequests(RingQueue& queue) const;
  CqeResult handleCqe(RingQueue& queue, const io_uring_cqe& cqe) const;
  void registerOutstandingEntry(uint64_t unique, RingEntry& entry) const;
  RingEntry& takeOutstandingEntry(uint64_t unique) const;
  void submitCommitAndFetch(RingEntry& entry) const;
  static fuse_out_header& getReplyHeader(RingEntry& entry);
  static fuse_uring_ent_in_out& getRingEntryInOut(RingEntry& entry);
  void destroyRingPool() noexcept;
  static void* allocatePageAlignedBuffer(size_t size);

  mutable folly::Synchronized<std::unordered_map<uint64_t, RingEntry*>>
      outstandingEntries_;
  mutable folly::once_flag sessionInitFlag_;
  mutable std::atomic<size_t> nextQueueId_{0};
#endif
  uint32_t queueDepth_;
};

} // namespace facebook::eden

#endif
