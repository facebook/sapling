/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/File.h>
#include <folly/Range.h>
#include <optional>

#include "eden/common/utils/FileOffset.h"
#include "eden/fs/inodes/FileContentStore.h"
#include "eden/fs/inodes/InodeCatalog.h"
#include "eden/fs/inodes/lmdbcatalog/LMDBStoreInterface.h"
#include "eden/fs/inodes/overlay/OverlayChecker.h"
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
class LMDBFileContentStore;

class LMDBInodeCatalog : public InodeCatalog {
 public:
  explicit LMDBInodeCatalog(LMDBFileContentStore* core) : core_(core) {}

  ~LMDBInodeCatalog() override {}

  LMDBInodeCatalog(const LMDBInodeCatalog&) = delete;
  LMDBInodeCatalog& operator=(const LMDBInodeCatalog&) = delete;

  LMDBInodeCatalog(LMDBInodeCatalog&&) = delete;
  LMDBInodeCatalog& operator=(LMDBInodeCatalog&&) = delete;

  bool supportsSemanticOperations() const override {
    return false;
  }

  void maintenance() override;

  std::vector<InodeNumber> getAllParentInodeNumbers() override;

  std::optional<InodeNumber> initOverlay(
      bool createIfNonExisting,
      bool bypassLockFile = false) override;

  void close(std::optional<InodeNumber> nextInodeNumber) override;

  bool initialized() const override;

  std::optional<overlay::OverlayDir> loadOverlayDir(
      InodeNumber inodeNumber) override;

  std::optional<overlay::OverlayDir> loadAndRemoveOverlayDir(
      InodeNumber inodeNumber) override;

  void saveOverlayDir(InodeNumber inodeNumber, overlay::OverlayDir&& odir)
      override;

  void saveOverlayDir(InodeNumber inodeNumber, std::string&& odir);

  void removeOverlayDir(InodeNumber inodeNumber) override;

  bool hasOverlayDir(InodeNumber inodeNumber) override;

  InodeNumber nextInodeNumber() override;

  InodeNumber scanLocalChanges(
      std::shared_ptr<const EdenConfig> config,
      AbsolutePathPiece mountPath,
      bool windowsSymlinksEnabled,
      InodeCatalog::LookupCallback& callback) override;

  std::optional<fsck::InodeInfo> loadInodeInfo(InodeNumber number) override;

 private:
  LMDBFileContentStore* core_;
};
} // namespace facebook::eden
