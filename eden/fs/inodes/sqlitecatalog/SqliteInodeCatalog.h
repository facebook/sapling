/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>
#include <optional>

#include "eden/fs/inodes/InodeCatalog.h"
#include "eden/fs/inodes/overlay/OverlayChecker.h"
#include "eden/fs/inodes/sqlitecatalog/SqliteTreeStore.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/utils/ImmediateFuture.h"
#include "eden/fs/utils/PathFuncs.h"

namespace folly {
class File;
}

namespace facebook::eden {

class EdenConfig;
namespace overlay {
class OverlayDir;
}
struct InodeNumber;
class StructuredLogger;

class SqliteInodeCatalog : public InodeCatalog {
 public:
  explicit SqliteInodeCatalog(
      AbsolutePathPiece path,
      std::shared_ptr<StructuredLogger> logger,
      SqliteTreeStore::SynchronousMode mode =
          SqliteTreeStore::SynchronousMode::Normal);

  explicit SqliteInodeCatalog(std::unique_ptr<SqliteDatabase> store)
      : store_(std::move(store)) {}

  ~SqliteInodeCatalog() override {}

  SqliteInodeCatalog(const SqliteInodeCatalog&) = delete;
  SqliteInodeCatalog& operator=(const SqliteInodeCatalog&) = delete;

  SqliteInodeCatalog(SqliteInodeCatalog&&) = delete;
  SqliteInodeCatalog& operator=(SqliteInodeCatalog&&) = delete;

  bool supportsSemanticOperations() const override {
    return true;
  }

  std::vector<InodeNumber> getAllParentInodeNumbers() override;

  std::optional<InodeNumber> initOverlay(
      bool createIfNonExisting,
      bool bypassLockFile = false) override;

  void close(std::optional<InodeNumber> nextInodeNumber) override;

  bool initialized() const override {
    return initialized_;
  }

  std::optional<overlay::OverlayDir> loadOverlayDir(
      InodeNumber inodeNumber) override;
  std::optional<overlay::OverlayDir> loadAndRemoveOverlayDir(
      InodeNumber inodeNumber) override;

  void saveOverlayDir(InodeNumber inodeNumber, overlay::OverlayDir&& odir)
      override;

  void removeOverlayDir(InodeNumber inodeNumber) override;

  bool hasOverlayDir(InodeNumber inodeNumber) override;

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

  InodeNumber nextInodeNumber() override;

  /**
   * Scan filesystem changes when EdenFS is not running. This is only required
   * on Windows as ProjectedFS allows user to make changes under certain
   * directory when EdenFS is not running.
   */
  InodeNumber scanLocalChanges(
      std::shared_ptr<const EdenConfig> config,
      AbsolutePathPiece mountPath,
      bool windowsSymlinksEnabled,
      InodeCatalog::LookupCallback& callback) override;

  void maintenance() override {
    store_.maintenance();
  }

  std::optional<fsck::InodeInfo> loadInodeInfo(InodeNumber number) override;

 private:
  SqliteTreeStore store_;

  bool initialized_ = false;
};
} // namespace facebook::eden
