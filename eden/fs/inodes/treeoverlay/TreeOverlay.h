/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>
#include <optional>

#include "eden/fs/inodes/IOverlay.h"
#include "eden/fs/inodes/treeoverlay/TreeOverlayStore.h"

namespace folly {
class File;
}

namespace facebook::eden {

namespace overlay {
class OverlayDir;
}
struct InodeNumber;

class TreeOverlay : public IOverlay {
 public:
  explicit TreeOverlay(
      AbsolutePathPiece path,
      TreeOverlayStore::SynchronousMode mode =
          TreeOverlayStore::SynchronousMode::Normal);

  explicit TreeOverlay(std::unique_ptr<SqliteDatabase> store)
      : store_(std::move(store)) {}

  ~TreeOverlay() override {}

  TreeOverlay(const TreeOverlay&) = delete;
  TreeOverlay& operator=(const TreeOverlay&) = delete;

  TreeOverlay(TreeOverlay&&) = delete;
  TreeOverlay& operator=(TreeOverlay&&) = delete;

  bool supportsSemanticOperations() const override {
    return true;
  }

  std::optional<InodeNumber> initOverlay(bool createIfNonExisting) override;

  void close(std::optional<InodeNumber> nextInodeNumber) override;

  bool initialized() const override {
    return initialized_;
  }

  const AbsolutePath& getLocalDir() const override;

  std::optional<overlay::OverlayDir> loadOverlayDir(
      InodeNumber inodeNumber) override;
  std::optional<overlay::OverlayDir> loadAndRemoveOverlayDir(
      InodeNumber inodeNumber) override;

  void saveOverlayDir(InodeNumber inodeNumber, overlay::OverlayDir&& odir)
      override;

#ifndef _WIN32
  folly::File createOverlayFile(
      InodeNumber inodeNumber,
      folly::ByteRange contents) override;

  folly::File createOverlayFile(
      InodeNumber inodeNumber,
      const folly::IOBuf& contents) override;

  folly::File openFile(InodeNumber inodeNumber, folly::StringPiece headerId)
      override;

  folly::File openFileNoVerify(InodeNumber inodeNumber) override;

  struct statfs statFs() const override;
#endif

  void removeOverlayData(InodeNumber inodeNumber) override;

  bool hasOverlayData(InodeNumber inodeNumber) override;

  void addChild(
      InodeNumber parent,
      PathComponentPiece name,
      overlay::OverlayEntry entry) override;

  void removeChild(InodeNumber parent, PathComponentPiece childName) override;

  bool hasChild(InodeNumber parent, PathComponentPiece childName) override;

  void renameChild(
      InodeNumber src,
      InodeNumber dst,
      PathComponentPiece srcName,
      PathComponentPiece dstName) override;

  InodeNumber nextInodeNumber();

  /**
   * Scan filesystem changes when EdenFS is not running. This is only required
   * on Windows as ProjectedFS allows user to make changes under certain
   * directory when EdenFS is not running.
   */
  InodeNumber scanLocalChanges(AbsolutePathPiece mountPath);

  void maintenance() override {
    store_.maintenance();
  }

 private:
  AbsolutePath path_;

  TreeOverlayStore store_;

  bool initialized_ = false;
};
} // namespace facebook::eden
