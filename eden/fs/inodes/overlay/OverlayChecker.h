/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include <memory>
#include <optional>

#include <folly/small_vector.h>

#include "eden/fs/fuse/InodeNumber.h"
#include "eden/fs/inodes/overlay/gen-cpp2/overlay_types.h"
#include "eden/fs/utils/PathFuncs.h"

namespace folly {
class File;
}

namespace facebook {
namespace eden {

class FsOverlay;

/**
 * OverlayChecker performs "fsck" operations on the on-disk overlay data.
 *
 * This class scans the on-disk data for errors, and repairs problems that are
 * found.
 */
class OverlayChecker {
 public:
  class Error {
   public:
    virtual ~Error() {}
    virtual std::string getMessage(OverlayChecker* checker) const = 0;
  };

  /**
   * Create a new OverlayChecker.
   *
   * The OverlayChecker stores a raw pointer to the FsOverlay for the duration
   * of the check operation.  The caller is responsible for ensuring that the
   * FsOverlay object exists for at least as long as the OverlayChecker object.
   */
  OverlayChecker(FsOverlay* fs, std::optional<InodeNumber> nextInodeNumber);

  ~OverlayChecker();

  /**
   * Scan the overlay for problems.
   */
  void scanForErrors();

  /**
   * Attempt to repair the errors that were found by scanForErrors().
   */
  void repairErrors();

  /**
   * Log the errors that were found by scanForErrors(), without fixing them.
   *
   * The repairErrors() call will also handle logging the errors.  The
   * logErrors() method only needs to be called when doing a dry-run where you
   * want to report errors without attempting to fix any problems.
   */
  void logErrors();

  /**
   * Return a reference to the list of errors.
   *
   * scanForErrors() should be called first to populate the error list.
   */
  const std::vector<std::unique_ptr<Error>>& getErrors() const {
    return errors_;
  }

  /**
   * Get the correct next inode number that was computed by scanForErrors()
   */
  InodeNumber getNextInodeNumber() const {
    return InodeNumber(maxInodeNumber_ + 1);
  }

 private:
  class ShardDirectoryEnumerationError;
  class UnexpectedOverlayFile;
  class UnexpectedInodeShard;
  class InodeDataError;
  class MissingMaterializedInode;
  class OrphanInode;
  class HardLinkedInode;
  class BadNextInodeNumber;

  enum class InodeType {
    File,
    Dir,
    Error,
  };
  struct InodeInfo {
    InodeInfo(InodeNumber num, InodeType t) : number(num), type(t) {}

    void addParent(InodeNumber parent) {
      parents.push_back(parent);
    }

    InodeNumber number;
    InodeType type{InodeType::Error};
    overlay::OverlayDir children;
    folly::small_vector<InodeNumber, 1> parents;
  };

  OverlayChecker(OverlayChecker const&) = delete;
  OverlayChecker& operator=(OverlayChecker const&) = delete;

  // Compute the path to a given child inode number in a parent directory
  RelativePath computePath(InodeNumber parent, PathComponentPiece child) const;
  // Compute the path to a given child inode number in a parent directory
  RelativePath computePath(InodeNumber parent, InodeNumber child) const;

  using ShardID = uint32_t;
  void readInodes();
  void readInodeSubdir(const AbsolutePath& path, ShardID shardID);
  void loadInode(InodeNumber number, ShardID shardID);
  std::unique_ptr<InodeInfo> loadInodeInfo(InodeNumber number);
  overlay::OverlayDir loadDirectoryChildren(folly::File& file);

  void linkInodeChildren();
  void scanForParentErrors();
  void checkNextInodeNumber();

  template <typename ErrorType, typename... Args>
  void addError(Args&&... args) {
    addError(std::make_unique<ErrorType>(std::forward<Args>(args)...));
  }
  void addError(std::unique_ptr<Error> error);

  void updateMaxInodeNumber(InodeNumber number) {
    if (number.get() > maxInodeNumber_) {
      maxInodeNumber_ = number.get();
    }
  }

  FsOverlay* const fs_;
  std::optional<InodeNumber> loadedNextInodeNumber_;
  std::unordered_map<InodeNumber, std::unique_ptr<InodeInfo>> inodes_;
  std::vector<std::unique_ptr<Error>> errors_;
  uint64_t maxInodeNumber_{0};
};

} // namespace eden
} // namespace facebook
