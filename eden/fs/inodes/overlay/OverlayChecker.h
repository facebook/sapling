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

  /**
   * A structure to represent best-effort computed paths for inodes.
   *
   * We cannot always compute the full path to some inodes if some of their
   * ancestors have been unlinked or orphaned.
   *
   * If we can compute the full path to an inode, parent will be kRootNodeId.
   * Otherwise, parent will be the inode number for the first ancestor that is
   * unlinked or orphaned (such that we cannot determine its path).
   *
   * path will be the path to this inode, relative to parent.
   * path may be empty if computePath() was called on an orphaned inode.
   */
  struct PathInfo {
    explicit PathInfo(InodeNumber number) : parent(number) {}
    explicit PathInfo(const PathInfo& parentInfo, PathComponentPiece child)
        : parent(parentInfo.parent), path(parentInfo.path + child) {}

    std::string toString() const;

    InodeNumber parent{kRootNodeId};
    RelativePath path;
  };

  /**
   * Compute the path to a given inode.
   *
   * scanForErrors() must have been called first to scan the inode data.
   */
  PathInfo computePath(InodeNumber number);
  PathInfo computePath(InodeNumber parent, PathComponentPiece child);

  /**
   * Compute the path to a given child inode number in a parent directory.
   *
   * This version is primarily useful only when there are hard links and you
   * wish to identify a specific path to the linked child inode.
   */
  PathInfo computePath(InodeNumber parent, InodeNumber child);

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

  PathInfo computePath(const InodeInfo& info);
  PathComponent findChildName(const InodeInfo& parentInfo, InodeNumber child);
  template <typename Fn>
  PathInfo cachedPathComputation(InodeNumber number, Fn&& fn);

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

  std::unordered_map<InodeNumber, PathInfo> pathCache_;
};

} // namespace eden
} // namespace facebook
