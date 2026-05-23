/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/fuse/IoUringFuseTransport.h"

#ifdef __linux__
#include <sys/eventfd.h>
#endif
#include <unistd.h>
#include <cstring>
#include <stdexcept>
#include <system_error>

#include <fmt/core.h>
#include <folly/logging/xlog.h>

#ifdef __linux__
#ifndef IORING_SETUP_SQE128
#error \
    "FUSE io_uring transport requires liburing support for IORING_SETUP_SQE128"
#endif
#endif

namespace facebook::eden {

namespace {

[[noreturn]] void throwIoUringNotImplemented(
    const char* method,
    uint32_t queueDepth) {
  throw std::runtime_error(
      fmt::format(
          "IoUringFuseTransport::{} not implemented (queueDepth={})",
          method,
          queueDepth));
}

} // namespace

IoUringFuseTransport::IoUringFuseTransport(uint32_t queueDepth)
    : queueDepth_{queueDepth} {}

IoUringFuseTransport::~IoUringFuseTransport() {
#ifdef __linux__
  destroyRingPool();
#endif
}

#ifdef __linux__
IoUringFuseTransport::RingQueue::RingQueue() {
  ring.ring_fd = -1;
}

IoUringFuseTransport::RingQueue::~RingQueue() noexcept {
  reset();
}

IoUringFuseTransport::RingQueue::RingQueue(RingQueue&& other) noexcept
    : pool{other.pool},
      queueId{other.queueId},
      eventFd{other.eventFd},
      requestHeaderSize{other.requestHeaderSize},
      ring{other.ring},
      ringInitialized{other.ringInitialized},
      entries{std::move(other.entries)} {
  other.resetMovedFrom();
}

IoUringFuseTransport::RingQueue& IoUringFuseTransport::RingQueue::operator=(
    RingQueue&& other) noexcept {
  if (this == &other) {
    return *this;
  }

  reset();

  pool = other.pool;
  queueId = other.queueId;
  eventFd = other.eventFd;
  requestHeaderSize = other.requestHeaderSize;
  ring = other.ring;
  ringInitialized = other.ringInitialized;
  entries = std::move(other.entries);

  other.resetMovedFrom();

  return *this;
}

void IoUringFuseTransport::RingQueue::resetMovedFrom() noexcept {
  pool = nullptr;
  queueId = 0;
  eventFd = -1;
  requestHeaderSize = sizeof(fuse_uring_req_header);
  ring = {};
  ring.ring_fd = -1;
  ringInitialized = false;
}

void IoUringFuseTransport::RingQueue::reset() noexcept {
  if (ringInitialized) {
    io_uring_queue_exit(&ring);
    ringInitialized = false;
    ring = {};
    ring.ring_fd = -1;
  }

  if (eventFd >= 0) {
    if (close(eventFd) != 0) {
      const auto savedErrno = errno;
      XLOGF(
          WARN,
          "failed to close io_uring eventfd for queue {}: errno={}",
          queueId,
          savedErrno);
    }
    eventFd = -1;
  }
}

void IoUringFuseTransport::initializeRingPool(
    size_t queueCount,
    size_t maxRequestPayloadSize,
    int fuseFd) {
  auto ringPool = std::make_unique<RingPool>();
  ringPool->queueDepth = queueDepth_;
  ringPool->maxRequestPayloadSize = maxRequestPayloadSize;
  ringPool->queues.resize(queueCount);

  for (size_t queueId = 0; queueId < queueCount; ++queueId) {
    auto& queue = ringPool->queues[queueId];
    queue.pool = ringPool.get();
    queue.queueId = queueId;
    queue.entries.resize(queueDepth_);
    initializeQueue(queue, fuseFd);
  }

  ringPool_ = std::move(ringPool);
}

void IoUringFuseTransport::initializeQueue(RingQueue& queue, int fuseFd) const {
  queue.eventFd = eventfd(0, EFD_CLOEXEC | EFD_NONBLOCK);
  if (queue.eventFd < 0) {
    const auto savedErrno = errno;
    throw std::system_error(
        savedErrno,
        std::generic_category(),
        fmt::format(
            "failed to create io_uring eventfd for queue {}", queue.queueId));
  }

  io_uring_params params = {};
  auto depth = static_cast<unsigned>(queueDepth_ + 1);
  params.flags = IORING_SETUP_CQSIZE | IORING_SETUP_SQE128;
  params.cq_entries = depth * 2;

  auto rc = io_uring_queue_init_params(depth, &queue.ring, &params);
  if (rc != 0) {
    throw std::system_error(
        -rc,
        std::generic_category(),
        fmt::format("failed to initialize io_uring queue {}", queue.queueId));
  }
  queue.ringInitialized = true;

  int files[1] = {fuseFd};
  rc = io_uring_register_files(&queue.ring, files, 1);
  if (rc != 0) {
    throw std::system_error(
        -rc,
        std::generic_category(),
        fmt::format(
            "failed to register /dev/fuse with io_uring queue {}",
            queue.queueId));
  }
}

void IoUringFuseTransport::initializeEntryBuffers(
    RingQueue& queue,
    RingEntry& entry) const {
  entry.pool = queue.pool;
  entry.queueId = queue.queueId;
  entry.requestHeaderStorage.reset(
      allocatePageAlignedBuffer(queue.requestHeaderSize));
  entry.payloadStorage.reset(
      allocatePageAlignedBuffer(queue.pool->maxRequestPayloadSize));

  entry.requestHeader =
      static_cast<fuse_uring_req_header*>(entry.requestHeaderStorage.get());
  entry.payload = entry.payloadStorage.get();
  entry.payloadSize = queue.pool->maxRequestPayloadSize;
  entry.requestCommitId = 0;

  entry.iov[0].iov_base = entry.requestHeader;
  entry.iov[0].iov_len = queue.requestHeaderSize;
  entry.iov[1].iov_base = entry.payload;
  entry.iov[1].iov_len = entry.payloadSize;
}

void IoUringFuseTransport::destroyRingPool() noexcept {
  ringPool_.reset();
}

void* IoUringFuseTransport::allocatePageAlignedBuffer(size_t size) {
  if (size == 0) {
    throw std::invalid_argument(
        "io_uring page-aligned buffer size must be non-zero");
  }

  void* ptr = nullptr;
  auto rc = posix_memalign(&ptr, static_cast<size_t>(getpagesize()), size);
  if (rc != 0) {
    throw std::system_error(
        rc,
        std::generic_category(),
        fmt::format(
            "failed to allocate page-aligned io_uring buffer of size {}",
            size));
  }

  if (!ptr) {
    throw std::runtime_error(
        fmt::format(
            "posix_memalign returned null for io_uring buffer of size {}",
            size));
  }

  return ptr;
}
#endif

const char* IoUringFuseTransport::getName() const {
  return "io_uring";
}

ssize_t IoUringFuseTransport::readInitPacket(
    int /* fd */,
    void* /* buf */,
    size_t /* size */) const {
  // Not implemented: FUSE_INIT still uses the classic /dev/fuse path.
  throwIoUringNotImplemented("readInitPacket", queueDepth_);
}

void IoUringFuseTransport::processSession(FuseChannel& /* channel */) {
  // Not implemented: io_uring request processing will be added in a follow-up
  // diff.
  throwIoUringNotImplemented("processSession", queueDepth_);
}

void IoUringFuseTransport::replyError(
    FuseChannel& /* channel */,
    const fuse_in_header& /* request */,
    int /* errorCode */) const {
  // Not implemented: io_uring reply submission will be added in a follow-up
  // diff.
  throwIoUringNotImplemented("replyError", queueDepth_);
}

void IoUringFuseTransport::sendRawReply(
    FuseChannel& /* channel */,
    const iovec[] /* iov */,
    size_t /* count */) const {
  // Not implemented: io_uring reply submission will be added in a follow-up
  // diff.
  throwIoUringNotImplemented("sendRawReply", queueDepth_);
}

} // namespace facebook::eden

#endif
