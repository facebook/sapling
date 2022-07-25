/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/treeoverlay/BufferedTreeOverlay.h"

#include <folly/logging/xlog.h>
#include <folly/system/ThreadName.h>

#include "eden/fs/inodes/treeoverlay/TreeOverlay.h"

namespace facebook::eden {

BufferedTreeOverlay::BufferedTreeOverlay(
    AbsolutePathPiece path,
    TreeOverlayStore::SynchronousMode mode)
    : TreeOverlay(path, mode) {
  workerThread_ = std::thread{[this] {
    folly::setThreadName("OverlayBuffer");
    processOnWorkerThread();
  }};
}

BufferedTreeOverlay::BufferedTreeOverlay(std::unique_ptr<SqliteDatabase> store)
    : TreeOverlay(std::move(store)) {
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
  }

  process([]() { return true; });

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
  std::vector<folly::Function<bool()>> work;

  for (;;) {
    work.clear();

    sem_.wait();

    {
      auto state = state_.lock();

      work.swap(state->work);
    }

    // processOnWorkerThread posts for every event added to the work queue, but
    // we wait on the semaphore only once per batch of events. For example, we
    // could post multiple times before this single wait, and we will pull and
    // process all the events on the queue for just a single wait. This makes
    // the semaphore more positive than it needs to be and is a performance cost
    // of extra spinning if left unaddressed.  sem_.wait() consumed one count,
    // but we know this semaphore was posted work.size() amount of times. Since
    // we will process all entries at once, rather than waking repeatedly,
    // consume the rest.
    if (work.size()) {
      // The - 1 here is to account for the inital semaphore wait. For example,
      // if only one event was added to the queue and the wait() was fulfilled,
      // work.size() would be 1, and we would not wait to try any extra waits,
      // so the -1 brings this to 0.
      (void)sem_.tryWait(work.size() - 1);
    }

    for (auto& event : work) {
      // event will return true if it was a stopping event, in which case the
      // thread should exit
      if (event()) {
        return;
      }
    }
  }
}

void BufferedTreeOverlay::process(folly::Function<bool()>&& fn) {
  auto state = state_.lock();
  // Don't enqueue work if a stop was already requested
  if (state->workerThreadStopRequested) {
    return;
  }
  state->work.push_back(std::move(fn));
  sem_.post();
}

} // namespace facebook::eden
