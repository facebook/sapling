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
#include <condition_variable>
#include <optional>
#include <vector>

#include "eden/fs/inodes/overlay/gen-cpp2/overlay_types.h"
#include "eden/fs/inodes/treeoverlay/TreeOverlay.h"

namespace facebook::eden {

struct InodeNumber;
class EdenConfig;

class BufferedTreeOverlay : public TreeOverlay {
 public:
  explicit BufferedTreeOverlay(
      AbsolutePathPiece path,
      const EdenConfig& config,
      TreeOverlayStore::SynchronousMode mode =
          TreeOverlayStore::SynchronousMode::Normal);

  explicit BufferedTreeOverlay(
      std::unique_ptr<SqliteDatabase> store,
      const EdenConfig& config);

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
  void process(folly::Function<bool()> fn, size_t captureSize);

  std::optional<overlay::OverlayDir> loadOverlayDir(
      InodeNumber inodeNumber) override;

  std::optional<overlay::OverlayDir> loadAndRemoveOverlayDir(
      InodeNumber inodeNumber) override;

  void saveOverlayDir(InodeNumber inodeNumber, overlay::OverlayDir&& odir)
      override;

  void removeOverlayData(InodeNumber inodeNumber) override;

  bool hasOverlayData(InodeNumber inodeNumber) override;

  void addChild(
      InodeNumber parent,
      PathComponentPiece name,
      overlay::OverlayEntry entry) override;

  void removeChild(InodeNumber parent, PathComponentPiece childName) override;

  void renameChild(
      InodeNumber src,
      InodeNumber dst,
      PathComponentPiece srcName,
      PathComponentPiece dstName) override;

 private:
  // Maximum size of the buffer in bytes
  size_t bufferSize_;
  struct State {
    bool workerThreadStopRequested = false;
    std::vector<std::pair<folly::Function<bool()>, size_t>> work;
    size_t totalSize = 0;
  };

  std::thread workerThread_;
  folly::Synchronized<State, std::mutex> state_;
  // Encodes the condition !state_.work.empty()
  std::condition_variable workCV_;
  // Encodes the condition state_.totalSize < bufferSize_
  std::condition_variable fullCV_;

  /**
   * Uses the workerThread_ to process writes to the TreeOverlay
   */
  void processOnWorkerThread();

  void stopWorkerThread();

  void flush();
};
} // namespace facebook::eden
