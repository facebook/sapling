/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Function.h>
#include <folly/Synchronized.h>
#include <folly/synchronization/LifoSem.h>
#include <optional>
#include <vector>

#include "eden/fs/inodes/treeoverlay/TreeOverlay.h"

namespace facebook::eden {

class BufferedTreeOverlay : public TreeOverlay {
 public:
  explicit BufferedTreeOverlay(
      AbsolutePathPiece path,
      TreeOverlayStore::SynchronousMode mode =
          TreeOverlayStore::SynchronousMode::Normal);

  explicit BufferedTreeOverlay(std::unique_ptr<SqliteDatabase> store);

  ~BufferedTreeOverlay() override;

  BufferedTreeOverlay(const BufferedTreeOverlay&) = delete;
  BufferedTreeOverlay& operator=(const BufferedTreeOverlay&) = delete;

  BufferedTreeOverlay(BufferedTreeOverlay&&) = delete;
  BufferedTreeOverlay& operator=(BufferedTreeOverlay&&) = delete;

  void close(std::optional<InodeNumber> inodeNumber) override;

  /**
   * Puts an folly::Function on a worker thread to be processed asynchronously.
   * The function should return a bool indicating whether or not the worker
   * thread should stop
   */
  void process(folly::Function<bool()>&& fn);

 private:
  struct State {
    bool workerThreadStopRequested = false;
    std::vector<folly::Function<bool()>> work;
  };

  // We use a LifoSem here due to the fact that it is faster than a std::mutex
  // condition vairable combination. It in general should be used in a case
  // in which performance is more important than fairness, and since this is
  // a single threaded worker, we don't care about fairness. See the header
  // file for this object for more information about its performance benefits.
  // Also, in general we use a semaphore here so the worker thread is not
  // spinning while the work queue is empty.
  folly::LifoSem sem_;
  std::thread workerThread_;
  folly::Synchronized<State, std::mutex> state_;

  /**
   * Uses the workerThread_ to process writes to the TreeOverlay
   */
  void processOnWorkerThread();

  void stopWorkerThread();
};
} // namespace facebook::eden
