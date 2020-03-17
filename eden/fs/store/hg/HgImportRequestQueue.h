/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Synchronized.h>
#include <condition_variable>
#include <mutex>
#include <vector>

#include "eden/fs/store/hg/HgImportRequest.h"

namespace facebook {
namespace eden {

class HgImportRequestQueue {
 public:
  explicit HgImportRequestQueue() {}

  /*
   * Puts an item into the queue.
   */
  void enqueue(HgImportRequest request);

  /*
   * Returns an item from the queue. This function only returns `std::nullopt`
   * while the queue is being destructed. This fucntion will block when there is
   * no item available in the queue.
   */
  std::optional<HgImportRequest> dequeue();

  void stop();

 private:
  HgImportRequestQueue(HgImportRequestQueue&&) = delete;
  HgImportRequestQueue& operator=(HgImportRequestQueue&&) = delete;

  struct State {
    bool running = true;
    std::vector<HgImportRequest> queue;
  };

  folly::Synchronized<State, std::mutex> state_;
  std::condition_variable queueCV_;
};

} // namespace eden
} // namespace facebook
