/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/sqlitecatalog/BufferedSqliteInodeCatalog.h"

#include <folly/futures/Future.h>
#include <folly/logging/xlog.h>
#include <folly/system/ThreadName.h>
#include <thrift/lib/cpp2/protocol/Serializer.h>
#include <cstddef>

#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/inodes/sqlitecatalog/SqliteInodeCatalog.h"

namespace facebook::eden {

BufferedSqliteInodeCatalog::BufferedSqliteInodeCatalog(
    AbsolutePathPiece path,
    const EdenConfig& config,
    SqliteTreeStore::SynchronousMode mode)
    : SqliteInodeCatalog(path, mode),
      bufferSize_{config.overlayBufferSize.getValue()} {
  workerThread_ = std::thread{[this] {
    folly::setThreadName("OverlayBuffer");
    processOnWorkerThread();
  }};
}

BufferedSqliteInodeCatalog::BufferedSqliteInodeCatalog(
    std::unique_ptr<SqliteDatabase> store,
    const EdenConfig& config)
    : SqliteInodeCatalog(std::move(store)),
      bufferSize_{config.overlayBufferSize.getValue()} {
  workerThread_ = std::thread{[this] {
    folly::setThreadName("OverlayBuffer");
    processOnWorkerThread();
  }};
}

void BufferedSqliteInodeCatalog::stopWorkerThread() {
  // Check first that a stop was not already requested
  {
    auto state = state_.lock();
    if (state->workerThreadStopRequested) {
      return;
    }
    state->workerThreadStopRequested = true;
    // Manually insert the shutdown request to avoid waiting for the enforced
    // size limit.
    state->work.push_back(
        std::make_unique<Work>([]() { return true; }, std::nullopt, 0));
    workCV_.notify_one();
    fullCV_.notify_all();
  }

  workerThread_.join();
}

BufferedSqliteInodeCatalog::~BufferedSqliteInodeCatalog() {
  stopWorkerThread();
}

void BufferedSqliteInodeCatalog::close(std::optional<InodeNumber> inodeNumber) {
  // We have to stop the thread here to flush all queued writes so they complete
  // before the overlay is closed.
  stopWorkerThread();
  SqliteInodeCatalog::close(inodeNumber);
}

void BufferedSqliteInodeCatalog::processOnWorkerThread() {
  // This vector should be considered read-only outside of the state_ lock. The
  // inflightOperation map contains raw pointers to the Work objects owned by
  // this vector, and other threads can read from that map, so we should not
  // modify the work vector without the state lock held.
  std::vector<std::unique_ptr<Work>> work;

  for (;;) {
    {
      auto state = state_.lock();
      state->inflightOperation.clear();
      work.clear();

      workCV_.wait(state.as_lock(), [&] { return !state->work.empty(); });

      // We explicitly don't check workerThreadStopRequested here since we rely
      // on stopWorkerThread() placing a shutdown request onto the work queue.
      // We don't want to exit early because we want to ensure all requests
      // prior to the shutdown request are processed before cleaning up.

      // Move the state work into the thread local work structure. The
      // thread local work structure will be cleared after processing.
      work.swap(state->work);
      // Move the waitingOperation into the inflightOperation structure. The
      // inflightOperation structure will be cleared after processing.
      state->inflightOperation.swap(state->waitingOperation);

      size_t workSize = 0;
      for (auto& event : work) {
        workSize += event->estimateIndirectMemoryUsage;
      }
      bool shouldNotify = state->totalSize >= bufferSize_;
      XCHECK_EQ(state->totalSize, workSize)
          << "totalSize bookkeeping diverged!";
      state->totalSize = 0;
      if (shouldNotify) {
        fullCV_.notify_all();
      }
      // In the worst case, it's possible twice the overlay memory could be
      // used. When the lock is released and waiters are notified, the new
      // buffer could be filled to capacity while the current one is being
      // processed
    }

    for (auto& event : work) {
      // event will return true if it was a stopping event, in which case the
      // thread should exit
      if (event->operation()) {
        return;
      }
    }
  }
}

void BufferedSqliteInodeCatalog::process(
    folly::Function<bool()> fn,
    size_t captureSize,
    InodeNumber operationKey,
    OperationType operationType,
    std::optional<overlay::OverlayDir>&& odir) {
  size_t size = captureSize + sizeof(fn) + fn.heapAllocatedMemory();
  std::unique_ptr<Work> work =
      std::make_unique<Work>(std::move(fn), std::move(odir), size);
  Operation operation = Operation{operationType, work.get()};

  auto state = state_.lock();
  fullCV_.wait(state.as_lock(), [&] {
    return state->totalSize < bufferSize_ || state->workerThreadStopRequested;
  });

  // Don't enqueue work if a stop was already requested
  if (state->workerThreadStopRequested) {
    return;
  }

  state->work.push_back(std::move(work));

  try {
    state->waitingOperation[operationKey] = operation;
  } catch (const std::exception& e) {
    XLOG(ERR) << "Failed to push work onto overlay buffer for inode "
              << operationKey << ": " << e.what();
    state->work.pop_back(); // no-throw guarantee since state->work is not empty
    // Immediately rethrow in the case of a failed enqueue. We don't need to
    // notify a waiting thread since there is no new waiting work.
    // The ISO C++ standard [container.requirements.general.11.1] states: If an
    // exception is thrown by a insert(), that function has no effects.
    throw;
  }

  state->totalSize += size;
  workCV_.notify_one();
}

void BufferedSqliteInodeCatalog::pause(folly::Future<folly::Unit>&& fut) {
  auto state = state_.lock();
  state->work.push_back(std::make_unique<Work>(
      [fut = std::move(fut)]() mutable {
        std::move(fut).wait();
        return false;
      },
      std::nullopt,
      0));
  workCV_.notify_one();
}

void BufferedSqliteInodeCatalog::flush() {
  // TODO: add fast path for read only use case where the work queue is empty
  // and the worker thread is idle
  folly::Promise<folly::Unit> promise;
  auto result = promise.getFuture();

  {
    auto state = state_.lock();
    state->work.push_back(std::make_unique<Work>(
        [promise = std::move(promise)]() mutable {
          promise.setValue(folly::unit);
          return false;
        },
        std::nullopt,
        0));
    workCV_.notify_one();
  }

  std::move(result).wait();
}

std::optional<overlay::OverlayDir> BufferedSqliteInodeCatalog::loadOverlayDir(
    InodeNumber inodeNumber) {
  {
    auto state = state_.lock();
    // check waiting work
    auto operationIter = state->waitingOperation.find(inodeNumber);
    if (operationIter != state->waitingOperation.end()) {
      if (operationIter->second.operationType == OperationType::Write) {
        return operationIter->second.work->odir.value();
      } else {
        return std::nullopt;
      }
    }
    // check inflight work
    operationIter = state->inflightOperation.find(inodeNumber);
    if (operationIter != state->inflightOperation.end()) {
      if (operationIter->second.operationType == OperationType::Write) {
        return operationIter->second.work->odir.value();
      } else {
        return std::nullopt;
      }
    }
  }

  return SqliteInodeCatalog::loadOverlayDir(inodeNumber);
}

std::optional<overlay::OverlayDir>
BufferedSqliteInodeCatalog::loadAndRemoveOverlayDir(InodeNumber inodeNumber) {
  {
    auto state = state_.lock();
    // check waiting work
    auto operationIter = state->waitingOperation.find(inodeNumber);
    if (operationIter != state->waitingOperation.end()) {
      if (operationIter->second.operationType == OperationType::Write) {
        overlay::OverlayDir odir = operationIter->second.work->odir.value();
        state.unlock();
        process(
            [this, inodeNumber]() {
              SqliteInodeCatalog::loadAndRemoveOverlayDir(inodeNumber);
              return false;
            },
            0,
            inodeNumber,
            OperationType::Remove);
        return std::move(odir);
      } else {
        return std::nullopt;
      }
    }
    // check inflight work
    operationIter = state->inflightOperation.find(inodeNumber);
    if (operationIter != state->inflightOperation.end()) {
      if (operationIter->second.operationType == OperationType::Write) {
        overlay::OverlayDir odir = operationIter->second.work->odir.value();
        state.unlock();
        process(
            [this, inodeNumber]() {
              SqliteInodeCatalog::loadAndRemoveOverlayDir(inodeNumber);
              return false;
            },
            0,
            inodeNumber,
            OperationType::Remove);
        return std::move(odir);
      } else {
        return std::nullopt;
      }
    }
  }

  return SqliteInodeCatalog::loadAndRemoveOverlayDir(inodeNumber);
}

void BufferedSqliteInodeCatalog::saveOverlayDir(
    InodeNumber inodeNumber,
    overlay::OverlayDir&& odir) {
  // Serializing and deserialzing the OverlayDir has similar runtime to
  // copying the structure directly but is more memory efficient.
  // This can be measured by running the OverlayDirSerializerBenchmark
  // using `/usr/bin/time -v`. If memory usage becomes an issue, it may be
  // worth serializing instead of moving the structure

  // captureSize is multiplied here since odir is copied to store both in the
  // folly::function and directly in the Work struct
  size_t captureSize = estimateIndirectMemoryUsage<
                           overlay::PathComponent,
                           overlay::OverlayEntry>(*odir.entries()) *
      2;

  overlay::OverlayDir odirTemp = odir;

  process(
      [this, inodeNumber, odir = std::move(odirTemp)]() mutable {
        SqliteInodeCatalog::saveOverlayDir(inodeNumber, std::move(odir));
        return false;
      },
      captureSize,
      inodeNumber,
      OperationType::Write,
      std::move(odir));
}

void BufferedSqliteInodeCatalog::removeOverlayDir(InodeNumber inodeNumber) {
  process(
      [this, inodeNumber]() {
        SqliteInodeCatalog::removeOverlayDir(inodeNumber);
        return false;
      },
      0,
      inodeNumber,
      OperationType::Remove);
}

bool BufferedSqliteInodeCatalog::hasOverlayDir(InodeNumber inodeNumber) {
  {
    auto state = state_.lock();
    // check waiting work
    auto operationIter = state->waitingOperation.find(inodeNumber);
    if (operationIter != state->waitingOperation.end()) {
      return operationIter->second.operationType == OperationType::Write;
    }
    // check inflight work
    operationIter = state->inflightOperation.find(inodeNumber);
    if (operationIter != state->inflightOperation.end()) {
      return operationIter->second.operationType == OperationType::Write;
    }
  }

  return SqliteInodeCatalog::hasOverlayDir(inodeNumber);
}

} // namespace facebook::eden
