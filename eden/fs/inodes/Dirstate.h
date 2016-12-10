/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once
#include <folly/Synchronized.h>
#include "eden/fs/inodes/DirstatePersistence.h"
#include "eden/fs/inodes/gen-cpp2/overlay_types.h"
#include "eden/fs/model/Tree.h"
#include "eden/utils/PathFuncs.h"

namespace {
class DirectoryDelta;
}

namespace facebook {
namespace eden {

class ClientConfig;
class EdenMount;
class InodeBase;
class ObjectStore;
class Tree;
class TreeInode;

namespace fusell {
class InodeBase;
class MountPoint;
}

/**
 *
 * Mercurial status code for a file. This is a function of:
 * 1. Whether there is a HgUserStatusDirective for the file.
 * 2. Whether the file exists on disk.
 * 3. Whether the file is already in the repo.
 * 4. Whether the file is matched by a pattern in .hgignore.
 */
enum class HgStatusCode {
  // PLEASE DO NOT ALPHA-SORT! We prefer CLEAN to correspond to 0, so these are
  // not alphabetically sorted. They are roughly ordered by expected frequency
  // of use.
  CLEAN,

  MODIFIED,
  ADDED,

  /** Indicates file has been marked for removal by the user. */
  REMOVED,

  /**
   * Indicates file is tracked by the repo, is not on disk, but has not been
   * marked for removal by the user.
   */
  MISSING,
  NOT_TRACKED,
  IGNORED,
};

const std::string& HgStatusCode_toString(HgStatusCode code);

class HgStatus {
 public:
  explicit HgStatus(std::unordered_map<RelativePath, HgStatusCode>&& statuses)
      : statuses_(statuses) {}

  /**
   * What happens if `path` is not in the internal statuses_ map? Should it
   * return CLEAN or something else?
   */
  HgStatusCode statusForPath(RelativePath path) const;

  size_t size() const {
    return statuses_.size();
  }

  bool operator==(const HgStatus& other) const {
    return statuses_ == other.statuses_;
  }

  /**
   * Returns something akin to what you would see when running `hg status`.
   * This is intended for debugging purposes: do not rely on the format of the
   * return value.
   */
  std::string toString() const;

  const std::unordered_map<RelativePath, HgStatusCode>* list() const {
    return &statuses_;
  }

 private:
  std::unordered_map<RelativePath, HgStatusCode> statuses_;
};

std::ostream& operator<<(std::ostream& os, const HgStatus& status);

struct DirstateRemoveError {
  RelativePath path;
  std::string errorMessage;
};
inline bool operator==(
    const DirstateRemoveError& lhs,
    const DirstateRemoveError& rhs) {
  return lhs.path == rhs.path && lhs.errorMessage == rhs.errorMessage;
}
inline bool operator!=(
    const DirstateRemoveError& lhs,
    const DirstateRemoveError& rhs) {
  return !(lhs == rhs);
}
std::ostream& operator<<(std::ostream& os, const DirstateRemoveError& status);

/**
 * This is designed to be a simple implemenation of an Hg dirstate. It's
 * "simple" in that every call to `getStatus()` walks the entire overlay to
 * determine which files have been added/modified/removed, and then compares
 * those files with the base commit to determine the appropriate Hg status code.
 *
 * Ideally, we would cache information between calls to `getStatus()` to make
 * this more efficient, but this seems like an OK place to start. Once we have
 * a complete implementation built that is supported by a battery of tests, then
 * we can try to optimize things.
 *
 * For the moment, let's assume that we have the invariant that every file that
 * has been modified since the "base commit" exists in the overlay. This means
 * that we do not allow a non-commit snapshot to remove files from the overlay.
 * Rather, the only time the overlay gets "cleaned up" is in response to a
 * commit or an update.
 *
 * This may not be what we want in the long run, but we need to get basic
 * Mercurial stuff working first before we can worry about snapshots.
 */
class Dirstate {
 public:
  explicit Dirstate(EdenMount* mount);
  ~Dirstate();

  /** Analogous to calling `hg status`. */
  std::unique_ptr<HgStatus> getStatus() const;

  /**
   * Analogous to `hg add <path>` where `<path>` is an ordinary file or symlink.
   */
  void add(RelativePathPiece path);

  /**
   * Analogous to `hg rm <path1> <path2> ...` where each `<path>` identifies a
   * file or directory in the manifest. (Note that the path may correspond to a
   * file that has already been removed from disk.)
   *
   * In Mercurial proper, `hg rm` can take multiple paths, some of which are
   * invalid arguments (they could be untracked files, for example). When this
   * happens:
   *
   * 1. `hg rm` is applied for the valid arguments.
   * 2. An error message is printed for each invalid argument.
   * 3. An exit code of 1 is returned.
   *
   * In order to support this behavior, this method can add entries to
   * errorsToReport, indicating error messages to present to the user. As such,
   * if this adds entries to errorsToReport, the corresponding exit code to
   * `hg rm` should be 1.
   */
  void removeAll(
      const std::vector<RelativePathPiece>* paths,
      bool force,
      std::vector<DirstateRemoveError>& errorsToReport);

 private:
  /**
   * Analogous to `hg rm <path>` where `<path>` is an ordinary file or symlink.
   */
  void remove(
      RelativePathPiece path,
      bool force,
      std::vector<DirstateRemoveError>& errorsToReport);

  /**
   * Compares the TreeEntries from a Tree in the base commit with those in the
   * current TreeInode. Differences are recorded in the provided delta.
   */
  void computeDelta(
      const std::vector<TreeEntry>* originalTreeEntries,
      TreeInode& current,
      DirectoryDelta& delta) const;

  /**
   * Recursively performs a depth-first traversal of the specified Tree, adding
   * all of the files under it as either REMOVED or MISSING to
   * copyOfUserDirectives, as appropriate.
   */
  void addDeletedEntries(
      const Tree* tree,
      RelativePathPiece pathToTree,
      std::unordered_map<RelativePath, HgStatusCode>* manifest,
      const std::unordered_map<RelativePath, overlay::UserStatusDirective>*
          userDirectives,
      std::unordered_map<RelativePathPiece, overlay::UserStatusDirective>*
          copyOfUserDirectives) const;

  /**
   * Note that EdenMount::getInodeBase() throws if path does not correspond to
   * an actual file. This helper function returns nullptr instead in that case.
   */
  std::shared_ptr<InodeBase> getInodeBaseOrNull(RelativePathPiece path) const;

  /** The EdenMount object that owns this Dirstate */
  EdenMount* const mount_{nullptr};
  DirstatePersistence persistence_;
  /**
   * Manifest of files in the working copy whose status is not CLEAN. These are
   * also referred to as "nonnormal" files.
   * TODO(mbolin): Consider StringKeyedMap instead of unordered_map.
   */
  folly::Synchronized<
      std::unordered_map<RelativePath, overlay::UserStatusDirective>>
      userDirectives_;
};
}
}
