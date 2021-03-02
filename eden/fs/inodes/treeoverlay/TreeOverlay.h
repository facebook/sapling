/*
 * Copyright (c) Facebook, Inc. and its affiliates.
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
  explicit TreeOverlay(AbsolutePathPiece path);

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

  void saveOverlayDir(InodeNumber inodeNumber, const overlay::OverlayDir& odir)
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

  void renameChild(
      InodeNumber src,
      InodeNumber dst,
      PathComponentPiece srcName,
      PathComponentPiece dstName) override;

 private:
  AbsolutePath path_;

  TreeOverlayStore store_;

  bool initialized_ = false;
};
} // namespace facebook::eden
