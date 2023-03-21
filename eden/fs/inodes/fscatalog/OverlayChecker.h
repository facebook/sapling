/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <memory>
#include <optional>

#include <folly/CppAttributes.h>

#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/utils/ImmediateFuture.h"
#include "eden/fs/utils/PathFuncs.h"

namespace folly {
class File;
}

namespace facebook::eden {

class InodeCatalog;
class FileContentStore;

/**
 * OverlayChecker performs "fsck" operations on the on-disk overlay data.
 *
 * This class scans the on-disk data for errors, and repairs problems that are
 * found.
 */
class OverlayChecker {
 private:
  class RepairState;

 public:
  class Error {
   public:
    virtual ~Error() {}
    virtual std::string getMessage(OverlayChecker* checker) const = 0;
    virtual bool repair(RepairState& repair) const = 0;
  };
  class RepairResult {
   public:
    AbsolutePath repairDir;
    uint32_t totalErrors{0};
    uint32_t fixedErrors{0};
  };

  using ProgressCallback = std::function<void(uint16_t)>;
  using LookupCallbackValue =
      std::variant<std::shared_ptr<const Tree>, TreeEntry>;
  using LookupCallback = std::function<ImmediateFuture<LookupCallbackValue>(
      const std::shared_ptr<const Tree>&,
      RelativePathPiece)>;

  /**
   * Create a new OverlayChecker.
   *
   * The OverlayChecker stores a raw pointer to the InodeCatalog and
   * FileContentStore for the duration of the check operation.  The caller is
   * responsible for ensuring that the InodeCatalog and FileContentStore objects
   * exist for at least as long as the OverlayChecker object.
   */
  OverlayChecker(
      InodeCatalog* inodeCatalog,
      FileContentStore* fcs,
      std::optional<InodeNumber> nextInodeNumber,
      LookupCallback& lookupCallback);

  ~OverlayChecker();

  /**
   * Scan the overlay for problems.
   */
  void scanForErrors(const ProgressCallback& progressCallback = [](auto) {});

  /**
   * Attempt to repair the errors that were found by scanForErrors().
   *
   * Returns std::nullopt if repairErrors() is called when there are no errors
   * to repair, otherwise returns a RepairResult.
   */
  std::optional<RepairResult> repairErrors();

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
  struct InodeInfo;

  OverlayChecker(OverlayChecker const&) = delete;
  OverlayChecker& operator=(OverlayChecker const&) = delete;

  // Get the InodeInfo object for the specified InodeNumber.
  // Returns null if no info for this inode number is present.
  // readInodes() must have been called for inode info to be populated.
  InodeInfo* FOLLY_NULLABLE getInodeInfo(InodeNumber number);

  ImmediateFuture<std::variant<std::shared_ptr<const Tree>, TreeEntry>> lookup(
      RelativePathPiece path);

  PathInfo computePath(const InodeInfo& info);
  PathComponent findChildName(const InodeInfo& parentInfo, InodeNumber child);
  template <typename Fn>
  PathInfo cachedPathComputation(InodeNumber number, Fn&& fn);

  using ShardID = uint32_t;
  void readInodes(const ProgressCallback& progressCallback = [](auto) {});
  void readInodeSubdir(const AbsolutePath& path, ShardID shardID);
  // loadInode and loadInodeInfo are called from a multi-threaded context, so
  // make them const so they can't accidentally mutate 'this'.
  std::optional<OverlayChecker::InodeInfo> loadInode(
      InodeNumber number,
      ShardID shardID,
      folly::Synchronized<std::vector<std::unique_ptr<Error>>>& errors) const;
  std::optional<OverlayChecker::InodeInfo> loadInodeInfo(
      InodeNumber number,
      folly::Synchronized<std::vector<std::unique_ptr<Error>>>& errors) const;

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

  struct Impl;

  std::unique_ptr<Impl> impl_;
  std::vector<std::unique_ptr<Error>> errors_;
  uint64_t maxInodeNumber_{kRootNodeId.get()};

  std::unordered_map<InodeNumber, PathInfo> pathCache_;
};

} // namespace facebook::eden
