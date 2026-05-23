/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/fuse/IoUringFuseTransport.h"
#include "eden/fs/fuse/FuseChannel.h"

#ifdef __linux__
#include <poll.h>
#include <sched.h>
#include <sys/eventfd.h>
#include <sys/sysinfo.h>
#endif

#include <unistd.h>
#include <algorithm>
#include <cerrno>
#include <cstring>
#include <limits>
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

#ifdef __linux__
void prepareUringCmdSqe(
    io_uring_sqe& sqe,
    uint32_t cmdOp,
    uint16_t queueId,
    uint64_t commitId,
    void* userData) {
  std::memset(&sqe, 0, sizeof(sqe));
  sqe.opcode = IORING_OP_URING_CMD;
  sqe.flags = IOSQE_FIXED_FILE;
  sqe.ioprio = 0;
  sqe.fd = 0;
  sqe.off = 0;
  sqe.rw_flags = 0;
  io_uring_sqe_set_data(&sqe, userData);
  sqe.cmd_op = cmdOp;
  sqe.__pad1 = 0;

  fuse_uring_cmd_req cmd = {};
  cmd.flags = 0;
  cmd.commit_id = commitId;
  cmd.qid = queueId;
  std::memcpy(sqe.cmd, &cmd, sizeof(cmd));
}

size_t roundUpToPageSize(size_t size) {
  const auto pageSize = static_cast<size_t>(getpagesize());
  return ((size + pageSize - 1) / pageSize) * pageSize;
}

void pinThreadToCpu(size_t cpu, size_t cpuCount) {
  if (cpuCount == 0) {
    return;
  }

  // cpu_set_t has a fixed capacity, so clamp the affinity domain to the CPUs
  // that CPU_SET() can actually encode before normalizing the queue id.
  const auto usableCpuCount =
      std::min(cpuCount, static_cast<size_t>(CPU_SETSIZE));
  cpu %= usableCpuCount;
  cpu_set_t cpuset;
  CPU_ZERO(&cpuset);
  CPU_SET(cpu, &cpuset);
  if (sched_setaffinity(0, sizeof(cpuset), &cpuset) != 0) {
    const auto savedErrno = errno;
    XLOGF(
        WARN,
        "failed to pin io_uring worker to cpu {}: {}",
        cpu,
        std::generic_category().message(savedErrno));
  }
}
#endif

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
  requestHeaderSize = roundUpToPageSize(sizeof(fuse_uring_req_header));
  pendingCommits =
      std::make_unique<folly::Synchronized<std::vector<RingEntry*>>>();
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
      entries{std::move(other.entries)},
      pendingCommits{std::move(other.pendingCommits)} {
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
  pendingCommits = std::move(other.pendingCommits);

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
    size_t maxRequestPayloadSize) {
  auto ringPool = std::make_unique<RingPool>();
  ringPool->queueDepth = queueDepth_;
  ringPool->maxRequestPayloadSize = maxRequestPayloadSize;
  ringPool->queues.resize(queueCount);

  for (size_t queueId = 0; queueId < queueCount; ++queueId) {
    auto& queue = ringPool->queues[queueId];
    queue.pool = ringPool.get();
    queue.queueId = queueId;
    queue.entries.resize(queueDepth_);
  }

  ringPool_ = std::move(ringPool);
}

void IoUringFuseTransport::initializeSession(FuseChannel& channel) {
  folly::call_once(sessionInitFlag_, [&] {
    const auto bufferSize = channel.getTransportBufferSize();
    const auto maxRequestPayloadSize =
        bufferSize > 4096 ? bufferSize - 4096 : bufferSize;
    const auto queueCount =
        getConfiguredQueueCount(channel.getTransportWorkerThreadCount());
    initializeRingPool(queueCount, maxRequestPayloadSize);
  });
}

void IoUringFuseTransport::initializeQueue(RingQueue& queue, int fuseFd) const {
  queue.eventFd = eventfd(0, EFD_CLOEXEC);
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
  entry.phase = RingEntry::Phase::Idle;

  entry.iov[0].iov_base = entry.requestHeader;
  entry.iov[0].iov_len = queue.requestHeaderSize;
  entry.iov[1].iov_base = entry.payload;
  entry.iov[1].iov_len = entry.payloadSize;
}

void IoUringFuseTransport::prepareFetchRequests(RingQueue& queue) const {
  for (auto& entry : queue.entries) {
    prepareFetchRequest(queue, entry);
  }

  const auto sqReady = io_uring_sq_ready(&queue.ring);
  if (sqReady != queue.entries.size()) {
    throw std::runtime_error(
        fmt::format(
            "io_uring queue {} prepared {} SQEs for {} fetch requests",
            queue.queueId,
            sqReady,
            queue.entries.size()));
  }

  prepareWakePollSqe(queue);
}

void IoUringFuseTransport::prepareWakePollSqe(RingQueue& queue) const {
  auto* sqe = io_uring_get_sqe(&queue.ring);
  if (!sqe) {
    throw std::runtime_error(
        fmt::format(
            "failed to get io_uring SQE for queue {} eventfd poll",
            queue.queueId));
  }

  io_uring_prep_poll_add(sqe, queue.eventFd, POLLIN);
  io_uring_sqe_set_data(sqe, &queue);
}

void IoUringFuseTransport::prepareFetchRequest(
    RingQueue& queue,
    RingEntry& entry) const {
  auto* sqe = io_uring_get_sqe(&queue.ring);
  if (!sqe) {
    throw std::runtime_error(
        fmt::format(
            "failed to get io_uring SQE for queue {} fetch registration",
            queue.queueId));
  }

  prepareUringCmdSqe(
      *sqe,
      FUSE_IO_URING_CMD_REGISTER,
      static_cast<uint16_t>(queue.queueId),
      /* commitId */ 0,
      &entry);
  markRegisterFetchSubmission(entry);
  sqe->addr = reinterpret_cast<uint64_t>(entry.iov.data());
  sqe->len = static_cast<__u32>(entry.iov.size());
}

IoUringFuseTransport::CqeResult IoUringFuseTransport::handleCqe(
    RingQueue& queue,
    const io_uring_cqe& cqe,
    bool stopRequested) const {
  auto* userData = io_uring_cqe_get_data(&cqe);
  if (isWakeEventCqe(queue, cqe, userData)) {
    return handleWakeEventCqe(queue);
  }
  if (cqe.res != 0) {
    if (cqe.res == -ECANCELED) {
      logCanceledCqe(queue, cqe, stopRequested, userData);
      if (shouldRecoverCanceledCommitAndFetchCqe(
              queue, userData, stopRequested)) {
        auto& entry = *static_cast<RingEntry*>(userData);
        recoverCanceledCommitAndFetchCqe(queue, entry);
        return {.action = CqeResult::Action::Ignored, .request = std::nullopt};
      }
    }

    if (shouldIgnoreCqeError(cqe.res, stopRequested)) {
      return {.action = CqeResult::Action::Ignored, .request = std::nullopt};
    }

    throw std::system_error(
        -cqe.res,
        std::generic_category(),
        fmt::format(
            "io_uring CQE failed on queue {} with result {}",
            queue.queueId,
            cqe.res));
  }

  if (!userData) {
    throw std::runtime_error(
        fmt::format(
            "io_uring CQE on queue {} had no request data", queue.queueId));
  }

  auto& entry = *static_cast<RingEntry*>(userData);
  auto& in = *reinterpret_cast<fuse_in_header*>(&entry.requestHeader->in_out);
  auto& ringInOut = entry.requestHeader->ring_ent_in_out;
  entry.requestCommitId = ringInOut.commit_id;
  if (entry.requestCommitId == 0) {
    throw std::runtime_error(
        fmt::format(
            "io_uring request on queue {} returned commit_id=0",
            queue.queueId));
  }

  if (in.len < sizeof(fuse_in_header)) {
    throw std::runtime_error(
        fmt::format(
            "io_uring request on queue {} was truncated: len={}",
            queue.queueId,
            in.len));
  }

  const auto argumentSize =
      static_cast<size_t>(in.len) - sizeof(fuse_in_header);
  const auto payloadSize = static_cast<size_t>(ringInOut.payload_sz);
  if (payloadSize > entry.payloadSize) {
    throw std::runtime_error(
        fmt::format(
            "io_uring request on queue {} has payload {} larger than buffer {}",
            queue.queueId,
            payloadSize,
            entry.payloadSize));
  }
  if (payloadSize > argumentSize) {
    throw std::runtime_error(
        fmt::format(
            "io_uring request on queue {} has payload {} larger than arg size {}",
            queue.queueId,
            payloadSize,
            argumentSize));
  }

  const auto opHeaderSize = argumentSize - payloadSize;
  if (opHeaderSize > sizeof(entry.requestHeader->op_in)) {
    throw std::runtime_error(
        fmt::format(
            "io_uring request on queue {} has op header {} larger than buffer {}",
            queue.queueId,
            opHeaderSize,
            sizeof(entry.requestHeader->op_in)));
  }

  DecodedRequest request;
  request.entry = &entry;
  request.header = in;
  request.arguments.resize(argumentSize);
  if (opHeaderSize > 0) {
    std::memcpy(
        request.arguments.data(), entry.requestHeader->op_in, opHeaderSize);
  }
  if (payloadSize > 0) {
    std::memcpy(
        request.arguments.data() + opHeaderSize, entry.payload, payloadSize);
  }

  markDecodedRequest(entry);
  return {
      .action = CqeResult::Action::DispatchRequest,
      .request = std::move(request)};
}

IoUringFuseTransport::CqeResult IoUringFuseTransport::handleWakeEventCqe(
    RingQueue& queue) const {
  eventfd_t value = 0;
  if (eventfd_read(queue.eventFd, &value) != 0) {
    throw std::system_error(
        errno,
        std::generic_category(),
        fmt::format(
            "failed to drain io_uring wake eventfd for queue {}",
            queue.queueId));
  }

  prepareWakePollSqe(queue);
  return {.action = CqeResult::Action::Ignored, .request = std::nullopt};
}

void IoUringFuseTransport::rejectDecodedRequestAfterStop(
    const DecodedRequest& request) const {
  auto& entry = *request.entry;
  auto& out = getReplyHeader(entry);
  auto& ringInOut = getRingEntryInOut(entry);

  out.error = -EIO;
  out.unique = request.header.unique;
  out.len = 0;
  ringInOut.payload_sz = 0;

  queueCommitAndFetch(entry);
}

void IoUringFuseTransport::registerOutstandingEntry(
    uint64_t unique,
    RingEntry& entry) const {
  auto outstandingEntries = outstandingEntries_.wlock();
  auto [it, inserted] = outstandingEntries->emplace(unique, &entry);
  if (!inserted) {
    throw std::runtime_error(
        fmt::format(
            "duplicate io_uring outstanding request unique={}", unique));
  }
  (void)it;
}

IoUringFuseTransport::RingEntry& IoUringFuseTransport::takeOutstandingEntry(
    uint64_t unique) const {
  auto outstandingEntries = outstandingEntries_.wlock();
  auto it = outstandingEntries->find(unique);
  if (it == outstandingEntries->end()) {
    throw std::runtime_error(
        fmt::format("unknown io_uring outstanding request unique={}", unique));
  }
  auto* entry = it->second;
  outstandingEntries->erase(it);
  return *entry;
}

void IoUringFuseTransport::queueCommitAndFetch(RingEntry& entry) const {
  auto& queue = entry.pool->queues.at(entry.queueId);
  {
    auto pendingCommits = queue.pendingCommits->wlock();
    pendingCommits->push_back(&entry);
  }
  notifyWorker(queue);
}

void IoUringFuseTransport::processPendingCommits(RingQueue& queue) const {
  std::vector<RingEntry*> pendingEntries;
  {
    auto pendingCommits = queue.pendingCommits->wlock();
    pendingEntries.swap(*pendingCommits);
  }

  if (pendingEntries.empty()) {
    return;
  }

  for (auto* entry : pendingEntries) {
    prepareCommitAndFetchSqe(queue, *entry);
  }

  auto rc = io_uring_submit(&queue.ring);
  if (rc < 0) {
    throw std::system_error(
        -rc,
        std::generic_category(),
        fmt::format(
            "failed to submit io_uring commit SQEs on queue {} while batching {} commits",
            queue.queueId,
            pendingEntries.size()));
  }
}

bool IoUringFuseTransport::hasPendingCommits(const RingQueue& queue) const {
  auto pendingCommits = queue.pendingCommits->rlock();
  return !pendingCommits->empty();
}

bool IoUringFuseTransport::shouldExitWorkerLoop(
    const FuseChannel& channel,
    const RingQueue& queue) const {
  if (!channel.isStopRequested()) {
    return false;
  }
  if (hasPendingCommits(queue)) {
    return false;
  }
  return !channel.hasPendingRequests();
}

void IoUringFuseTransport::notifyWorker(const RingQueue& queue) const {
  if (eventfd_write(queue.eventFd, 1) != 0) {
    throw std::system_error(
        errno,
        std::generic_category(),
        fmt::format(
            "failed to notify io_uring worker for queue {}", queue.queueId));
  }
}

void IoUringFuseTransport::prepareCommitAndFetchSqe(
    RingQueue& queue,
    RingEntry& entry) const {
  auto* sqe = io_uring_get_sqe(&queue.ring);
  if (!sqe) {
    throw std::runtime_error(
        fmt::format(
            "failed to get io_uring SQE for queue {} commit", entry.queueId));
  }

  prepareUringCmdSqe(
      *sqe,
      FUSE_IO_URING_CMD_COMMIT_AND_FETCH,
      static_cast<uint16_t>(entry.queueId),
      entry.requestCommitId,
      &entry);
  markCommitAndFetchSubmission(entry);
}

fuse_out_header& IoUringFuseTransport::getReplyHeader(RingEntry& entry) {
  return *reinterpret_cast<fuse_out_header*>(&entry.requestHeader->in_out);
}

fuse_uring_ent_in_out& IoUringFuseTransport::getRingEntryInOut(
    RingEntry& entry) {
  return entry.requestHeader->ring_ent_in_out;
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

bool IoUringFuseTransport::isTransientSubmitAndWaitError(int result) {
  return result == -EINTR || result == -EAGAIN;
}

bool IoUringFuseTransport::shouldRetrySubmitAndWaitError(
    int result,
    bool stopRequested) {
  return !stopRequested && isTransientSubmitAndWaitError(result);
}

bool IoUringFuseTransport::shouldIgnoreSubmitAndWaitError(
    int result,
    bool stopRequested) {
  return stopRequested && isTransientSubmitAndWaitError(result);
}

bool IoUringFuseTransport::shouldIgnoreCqeError(int result) {
  return result == -EINTR || result == -EOPNOTSUPP || result == -EAGAIN ||
      result == -ENOTCONN;
}

bool IoUringFuseTransport::shouldIgnoreCqeErrorDuringShutdown(int result) {
  return result == -ECANCELED;
}

bool IoUringFuseTransport::shouldIgnoreCqeError(
    int result,
    bool stopRequested) {
  return shouldIgnoreCqeError(result) ||
      (stopRequested && shouldIgnoreCqeErrorDuringShutdown(result));
}

bool IoUringFuseTransport::isWakeEventCqe(
    const RingQueue& queue,
    const io_uring_cqe& cqe,
    void* userData) {
  return cqe.res > 0 && userData == &queue;
}

void IoUringFuseTransport::markRegisterFetchSubmission(RingEntry& entry) const {
  entry.requestCommitId = 0;
  entry.phase = RingEntry::Phase::RegisterFetchInFlight;
}

void IoUringFuseTransport::markDecodedRequest(RingEntry& entry) const {
  entry.phase = RingEntry::Phase::RequestOutstanding;
}

void IoUringFuseTransport::markCommitAndFetchSubmission(
    RingEntry& entry) const {
  entry.phase = RingEntry::Phase::CommitAndFetchInFlight;
}

bool IoUringFuseTransport::shouldRecoverCanceledCommitAndFetchCqe(
    const RingQueue& queue,
    void* userData,
    bool stopRequested) const {
  if (stopRequested || !userData) {
    return false;
  }

  if (userData == &queue) {
    return false;
  }

  const auto& entry = *static_cast<RingEntry*>(userData);
  return entry.phase == RingEntry::Phase::CommitAndFetchInFlight;
}

void IoUringFuseTransport::recoverCanceledCommitAndFetchCqe(
    RingQueue& queue,
    RingEntry& entry) const {
  XLOGF(
      WARN,
      "recovering canceled io_uring commit_and_fetch CQE for queueId={} entry={} commitId={} phase={}",
      queue.queueId,
      static_cast<void*>(&entry),
      entry.requestCommitId,
      static_cast<int>(entry.phase));
  prepareFetchRequest(queue, entry);
}

bool IoUringFuseTransport::isEntryOutstanding(const RingEntry& entry) const {
  auto outstandingEntries = outstandingEntries_.rlock();
  for (const auto& [unique, candidate] : *outstandingEntries) {
    (void)unique;
    if (candidate == &entry) {
      return true;
    }
  }
  return false;
}

void IoUringFuseTransport::logCanceledCqe(
    const RingQueue& queue,
    const io_uring_cqe& cqe,
    bool stopRequested,
    void* userData) const {
  const auto isEventFd = userData == &queue;
  if (!userData || isEventFd) {
    XLOGF(
        ERR,
        "io_uring canceled CQE queueId={} stopRequested={} res={} flags=0x{:x} userData={} eventFd={} isEventFd={}",
        queue.queueId,
        stopRequested,
        cqe.res,
        cqe.flags,
        userData,
        queue.eventFd,
        isEventFd);
    return;
  }

  const auto& entry = *static_cast<RingEntry*>(userData);
  const auto& ringInOut = entry.requestHeader->ring_ent_in_out;
  const auto& bestEffortIn =
      *reinterpret_cast<const fuse_in_header*>(&entry.requestHeader->in_out);
  const auto& bestEffortOut =
      *reinterpret_cast<const fuse_out_header*>(&entry.requestHeader->in_out);
  XLOGF(
      ERR,
      "io_uring canceled CQE queueId={} stopRequested={} res={} flags=0x{:x} userData={} entry.queueId={} phase={} requestCommitId={} outstanding={} bestEffortIn.unique={} bestEffortIn.opcode={} bestEffortIn.len={} ringInOut.commitId={} ringInOut.payloadSz={} bestEffortOut.unique={} bestEffortOut.error={} bestEffortOut.len={}",
      queue.queueId,
      stopRequested,
      cqe.res,
      cqe.flags,
      userData,
      entry.queueId,
      static_cast<int>(entry.phase),
      entry.requestCommitId,
      isEntryOutstanding(entry),
      bestEffortIn.unique,
      bestEffortIn.opcode,
      bestEffortIn.len,
      ringInOut.commit_id,
      ringInOut.payload_sz,
      bestEffortOut.unique,
      bestEffortOut.error,
      bestEffortOut.len);
}

size_t IoUringFuseTransport::getConfiguredQueueCount(
    size_t defaultThreadCount) {
  const auto configuredCpuCount = get_nprocs_conf();
  if (configuredCpuCount <= 0) {
    return defaultThreadCount;
  }
  return static_cast<size_t>(configuredCpuCount);
}
#endif

const char* IoUringFuseTransport::getName() const {
  return "io_uring";
}

size_t IoUringFuseTransport::getWorkerThreadCount(
    size_t defaultThreadCount) const {
#ifdef __linux__
  return getConfiguredQueueCount(defaultThreadCount);
#else
  return defaultThreadCount;
#endif
}

void IoUringFuseTransport::requestStopWakeup() {
#ifdef __linux__
  if (!ringPool_) {
    return;
  }

  for (const auto& queue : ringPool_->queues) {
    try {
      notifyWorker(queue);
    } catch (const std::exception& ex) {
      XLOGF(
          ERR,
          "failed to wake io_uring queue {} during shutdown: {}",
          queue.queueId,
          ex.what());
    }
  }
#endif
}

ssize_t IoUringFuseTransport::readInitPacket(
    int /* fd */,
    void* /* buf */,
    size_t /* size */) const {
  // Not implemented: FUSE_INIT still uses the classic /dev/fuse path.
  throwIoUringNotImplemented("readInitPacket", queueDepth_);
}

void IoUringFuseTransport::processSession(FuseChannel& channel) {
#ifdef __linux__
  initializeSession(channel);

  const auto queueId = nextQueueId_.fetch_add(1, std::memory_order_acq_rel);
  if (!ringPool_ || queueId >= ringPool_->queues.size()) {
    throw std::runtime_error(
        fmt::format(
            "failed to assign io_uring queue {} (queue_count={})",
            queueId,
            ringPool_ ? ringPool_->queues.size() : 0));
  }

  auto& queue = ringPool_->queues[queueId];
  const auto myPid = getpid();
  const auto configuredCpuCount = get_nprocs_conf();
  if (configuredCpuCount > 0) {
    pinThreadToCpu(queue.queueId, static_cast<size_t>(configuredCpuCount));
  }

  // Match libfuse by initializing the queue and its entry buffers after CPU
  // pinning, so the per-queue memory follows the worker's CPU locality.
  initializeQueue(queue, channel.getFuseDeviceFd());
  for (auto& entry : queue.entries) {
    initializeEntryBuffers(queue, entry);
  }
  prepareFetchRequests(queue);
  auto rc = io_uring_submit(&queue.ring);
  if (rc < 0) {
    throw std::system_error(
        -rc,
        std::generic_category(),
        fmt::format(
            "failed to submit initial io_uring fetch SQEs for queue {}",
            queue.queueId));
  }

  while (true) {
    processPendingCommits(queue);
    if (shouldExitWorkerLoop(channel, queue)) {
      break;
    }

    rc = io_uring_submit_and_wait(&queue.ring, 1);
    if (rc < 0) {
      const auto stopRequested = channel.isStopRequested();
      if (shouldRetrySubmitAndWaitError(rc, stopRequested)) {
        continue;
      }
      if (shouldIgnoreSubmitAndWaitError(rc, stopRequested)) {
        if (shouldExitWorkerLoop(channel, queue)) {
          break;
        }
        continue;
      }
      throw std::system_error(
          -rc,
          std::generic_category(),
          fmt::format(
              "io_uring_submit_and_wait failed for queue {}", queue.queueId));
    }

    unsigned completed = 0;
    unsigned head = 0;
    io_uring_cqe* cqe = nullptr;
    io_uring_for_each_cqe(&queue.ring, head, cqe) {
      ++completed;
      if (!cqe) {
        throw std::runtime_error(
            fmt::format(
                "io_uring CQE iteration returned null on queue {}",
                queue.queueId));
      }
      auto result = handleCqe(queue, *cqe, channel.isStopRequested());
      switch (result.action) {
        case CqeResult::Action::DispatchRequest: {
          XCHECK(result.request.has_value());
          auto& request = *result.request;
          if (channel.isStopRequested()) {
            // COMMIT_AND_FETCH can return another request while shutdown is
            // draining. Recycle it with an error instead of dispatching new
            // work into FuseChannel after stop was requested.
            rejectDecodedRequestAfterStop(request);
            break;
          }
          registerOutstandingEntry(request.header.unique, *request.entry);
          channel.dispatchRequestFromTransport(
              request.header,
              folly::ByteRange{
                  request.arguments.data(), request.arguments.size()},
              myPid);
          break;
        }
        case CqeResult::Action::Ignored:
          break;
        case CqeResult::Action::StopRequested:
          channel.requestSessionExitFromTransport(
              FuseChannel::StopReason::UNMOUNTED);
          break;
      }
    }
    if (completed > 0) {
      io_uring_cq_advance(&queue.ring, completed);
    }

    processPendingCommits(queue);
  }
#else
  (void)channel;
  throwIoUringNotImplemented("processSession", queueDepth_);
#endif
}

void IoUringFuseTransport::replyError(
    FuseChannel& /* channel */,
    const fuse_in_header& request,
    int errorCode) const {
#ifdef __linux__
  auto& entry = takeOutstandingEntry(request.unique);
  auto& out = getReplyHeader(entry);
  auto& ringInOut = getRingEntryInOut(entry);

  out.error = -errorCode;
  out.unique = request.unique;
  out.len = 0;
  ringInOut.payload_sz = 0;

  queueCommitAndFetch(entry);
#else
  (void)request;
  (void)errorCode;
  throwIoUringNotImplemented("replyError", queueDepth_);
#endif
}

void IoUringFuseTransport::sendRawReply(
    FuseChannel& /* channel */,
    const iovec iov[],
    size_t count) const {
#ifdef __linux__
  if (count == 0) {
    throw std::runtime_error("io_uring reply requires at least one iovec");
  }

  const auto& sourceHeader =
      *static_cast<const fuse_out_header*>(iov[0].iov_base);
  auto& entry = takeOutstandingEntry(sourceHeader.unique);
  auto& out = getReplyHeader(entry);
  auto& ringInOut = getRingEntryInOut(entry);

  size_t payloadLength = 0;
  int error = sourceHeader.error;
  auto* payload = static_cast<uint8_t*>(entry.payload);
  for (size_t idx = 1; idx < count; ++idx) {
    if (iov[idx].iov_len > entry.payloadSize - payloadLength) {
      XLOGF(
          WARN,
          "io_uring reply payload exceeds buffer: unique={} iov_index={} iov_len={} copied_payload_length={} payload_buffer_size={}",
          sourceHeader.unique,
          idx,
          iov[idx].iov_len,
          payloadLength,
          entry.payloadSize);
      error = -EINVAL;
      payloadLength = 0;
      break;
    }

    std::memcpy(payload + payloadLength, iov[idx].iov_base, iov[idx].iov_len);
    payloadLength += iov[idx].iov_len;
  }

  if (payloadLength > std::numeric_limits<uint32_t>::max()) {
    XLOGF(
        WARN,
        "io_uring reply payload exceeds uint32_t limit: unique={} payload_length={}",
        sourceHeader.unique,
        payloadLength);
    error = -EINVAL;
    payloadLength = 0;
  }
  const auto payloadLength32 = static_cast<uint32_t>(payloadLength);

  out.error = error;
  out.unique = sourceHeader.unique;
  out.len = payloadLength32;
  ringInOut.payload_sz = payloadLength32;

  queueCommitAndFetch(entry);
#else
  (void)iov;
  (void)count;
  throwIoUringNotImplemented("sendRawReply", queueDepth_);
#endif
}

} // namespace facebook::eden

#endif
