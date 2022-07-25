/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/treeoverlay/BufferedTreeOverlay.h"

#include <folly/futures/Future.h>
#include <folly/logging/xlog.h>
#include <folly/system/ThreadName.h>
#include <thrift/lib/cpp2/protocol/Serializer.h>

#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/inodes/treeoverlay/TreeOverlay.h"

namespace facebook::eden {

// namespace detail {

// static inline size_t estimateIndirectMemoryUsage(
//     const std::map<overlay::PathComponent, overlay::OverlayEntry>& entries) {
//   // std::map is implemented using a red-black tree.

//   // Accumulate the estimated usage of the base nodes of the tree
// #if defined(_STL_TREE_H)
//   size_t usage = folly::goodMallocSize(sizeof(std::_Rb_tree_node<std::pair<
//                                                   const
//                                                   overlay::PathComponent,
//                                                   overlay::OverlayEntry>>)) *
//       entries.size();
// #elif defined(_XTREE_)
//   size_t usage =
//       folly::goodMallocSize(
//           sizeof(std::_Tree_node<
//                  std::pair<const overlay::PathComponent,
//                  overlay::OverlayEntry>, void*>)) *
//       entries.size();
// #elif defined(_LIBCPP___TREE)
//   size_t usage =
//       folly::goodMallocSize(
//           sizeof(std::__tree_node<
//                  std::pair<const overlay::PathComponent,
//                  overlay::OverlayEntry>, void*>)) *
//       entries.size();
// #endif

//   // Accumulate any indirect usage from the nodes
//   for (const auto& pair : entries) {
//     usage += facebook::eden::estimateIndirectMemoryUsage(std::get<0>(pair));
//     if (auto entryHash = std::get<1>(pair).get_hash()) {
//       usage += facebook::eden::estimateIndirectMemoryUsage(*entryHash);
//     }
//   }

//   return usage;
// }

// } // namespace detail

BufferedTreeOverlay::BufferedTreeOverlay(
    AbsolutePathPiece path,
    const EdenConfig& config,
    TreeOverlayStore::SynchronousMode mode)
    : TreeOverlay(path, mode),
      bufferSize_{config.overlayBufferSize.getValue()} {
  workerThread_ = std::thread{[this] {
    folly::setThreadName("OverlayBuffer");
    processOnWorkerThread();
  }};
}

BufferedTreeOverlay::BufferedTreeOverlay(
    std::unique_ptr<SqliteDatabase> store,
    const EdenConfig& config)
    : TreeOverlay(std::move(store)),
      bufferSize_{config.overlayBufferSize.getValue()} {
  workerThread_ = std::thread{[this] {
    folly::setThreadName("OverlayBuffer");
    processOnWorkerThread();
  }};
}

void BufferedTreeOverlay::stopWorkerThread() {
  // Check first that a stop was not already requested
  {
    auto state = state_.lock();
    if (state->workerThreadStopRequested) {
      return;
    }
    state->workerThreadStopRequested = true;
    // Manually insert the shutdown request to avoid waiting for the enforced
    // size limit.
    state->work.push_back(std::make_pair([]() { return true; }, 0));
    workCV_.notify_one();
  }

  workerThread_.join();
}

BufferedTreeOverlay::~BufferedTreeOverlay() {
  stopWorkerThread();
}

void BufferedTreeOverlay::close(std::optional<InodeNumber> inodeNumber) {
  // We have to stop the thread here to flush all queued writes so they complete
  // before the overlay is closed.
  stopWorkerThread();
  TreeOverlay::close(inodeNumber);
}

void BufferedTreeOverlay::processOnWorkerThread() {
  std::vector<std::pair<folly::Function<bool()>, size_t>> work;

  for (;;) {
    work.clear();

    {
      auto state = state_.lock();

      workCV_.wait(state.as_lock(), [&] { return !state->work.empty(); });

      // We explicitly don't check workerThreadStopRequested here since we rely
      // on stopWorkerThread() placing a shutdown request onto the work queue.
      // We don't want to exit early because we want to ensure all requests
      // prior to the shutdown request are processed before cleaning up.

      work.swap(state->work);

      size_t workSize = 0;
      for (auto& event : work) {
        workSize += event.second;
      }
      state->totalSize -= workSize;
    }

    for (auto& event : work) {
      // event will return true if it was a stopping event, in which case the
      // thread should exit
      if (event.first()) {
        return;
      }
    }
  }
}

void BufferedTreeOverlay::process(
    folly::Function<bool()> fn,
    size_t captureSize) {
  auto state = state_.lock();
  // Don't enqueue work if a stop was already requested
  if (state->workerThreadStopRequested) {
    return;
  }

  fullCV_.wait(state.as_lock(), [&] {
    return state->totalSize < bufferSize_ || state->workerThreadStopRequested;
  });

  if (state->workerThreadStopRequested) {
    return;
  }

  size_t size = captureSize + sizeof(fn) + fn.heapAllocatedMemory();
  state->work.push_back(std::make_pair(std::move(fn), size));
  state->totalSize += size;
  workCV_.notify_one();
}

void BufferedTreeOverlay::flush() {
  // TODO: add fast path for read only use case where the work queue is empty
  // and the worker thread is idle
  folly::Promise<folly::Unit> promise;
  auto result = promise.getFuture();

  process(
      [promise = std::move(promise)]() mutable {
        promise.setValue(folly::unit);
        return false;
      },
      0);

  std::move(result).wait();
}

std::optional<overlay::OverlayDir> BufferedTreeOverlay::loadOverlayDir(
    InodeNumber inodeNumber) {
  flush();

  return TreeOverlay::loadOverlayDir(inodeNumber);
}

std::optional<overlay::OverlayDir> BufferedTreeOverlay::loadAndRemoveOverlayDir(
    InodeNumber inodeNumber) {
  flush();

  return TreeOverlay::loadAndRemoveOverlayDir(inodeNumber);
}

void BufferedTreeOverlay::saveOverlayDir(
    InodeNumber inodeNumber,
    overlay::OverlayDir&& odir) {
  // Serializing and deserialzing the OverlayDir has similar runtime to
  // copying the structure directly but is more memory efficient.
  // This can be measured by running the OverlayDirSerializerBenchmark
  // using `/usr/bin/time -v`. If memory usage becomes an issue, it may be
  // worth serializing instead of moving the structure

  size_t captureSize = estimateIndirectMemoryUsage<
      overlay::PathComponent,
      overlay::OverlayEntry>(odir.get_entries());

  process(
      [this, inodeNumber, odir = std::move(odir)]() mutable {
        TreeOverlay::saveOverlayDir(inodeNumber, std::move(odir));
        return false;
      },
      captureSize);
}

void BufferedTreeOverlay::removeOverlayData(InodeNumber inodeNumber) {
  process(
      [this, inodeNumber]() {
        TreeOverlay::removeOverlayData(inodeNumber);
        return false;
      },
      0);
}

bool BufferedTreeOverlay::hasOverlayData(InodeNumber inodeNumber) {
  flush();

  return TreeOverlay::hasOverlayData(inodeNumber);
}

void BufferedTreeOverlay::addChild(
    InodeNumber parent,
    PathComponentPiece name,
    overlay::OverlayEntry entry) {
  process(
      [this, parent, name = name.copy(), entry = std::move(entry)]() {
        TreeOverlay::addChild(parent, name, entry);
        return false;
      },
      0);
}

void BufferedTreeOverlay::removeChild(
    InodeNumber parent,
    PathComponentPiece childName) {
  process(
      [this, parent, childName = childName.copy()]() {
        TreeOverlay::removeChild(parent, childName);
        return false;
      },
      0);
}

void BufferedTreeOverlay::renameChild(
    InodeNumber src,
    InodeNumber dst,
    PathComponentPiece srcName,
    PathComponentPiece dstName) {
  process(
      [this, src, dst, srcName = srcName.copy(), dstName = dstName.copy()]() {
        TreeOverlay::renameChild(src, dst, srcName, dstName);
        return false;
      },
      0);
}
} // namespace facebook::eden
