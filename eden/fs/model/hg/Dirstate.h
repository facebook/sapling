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
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/utils/PathFuncs.h"

namespace {
class DirectoryDelta;
}

namespace facebook {
namespace eden {

class TreeInode;

enum class HgStatusCode {
  // I think nothing in Dirstate.userChanges_ should ever have this state?
  // If so, it probably makes sense to remove it from the enum.
  CLEAN = 0,

  MODIFIED = 1,
  ADDED = 2,

  /** Indicates file has been marked for removal by the user. */
  REMOVED = 3,

  /**
   * Indicates file is tracked by the repo, is not on disk, but has not been
   * marked for removal by the user.
   */
  MISSING = 4,
  NOT_TRACKED = 5,
  IGNORED = 6,
};

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

 private:
  std::unordered_map<RelativePath, HgStatusCode> statuses_;
};

class DirstatePersistence {
 public:
  virtual ~DirstatePersistence() {}
  virtual void save(
      std::unordered_map<RelativePath, HgStatusCode>& userChanges) = 0;
};

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
  // TODO(mbolin): Frequently, the Dirstate should be "rehydrated" from an
  // on-disk version when rebooting Eden, so we should have a constructor that
  // reflects that.
  explicit Dirstate(
      std::shared_ptr<EdenMount> edenMount,
      std::unique_ptr<DirstatePersistence> persistence)
      : edenMount_(edenMount), persistence_(std::move(persistence)) {}

  /** Analogous to calling `hg status`. */
  std::unique_ptr<HgStatus> getStatus();

  /**
   * Analogous to `hg add <path>` where `<path>` is an ordinary file or symlink.
   */
  void add(RelativePathPiece path);

 private:
  /**
   * Sets the entry in the userChanges_ map, ensuring that the appropriate
   * invariants are maintained. Assuming this modifies the userChanges_ map, it
   * must also save the changes via the DirstatePersistence abstraction.
   */
  void applyUserStatusChange_(RelativePathPiece file, HgStatusCode code);

  void computeDelta(
      const Tree& original,
      TreeInode& current,
      DirectoryDelta& delta) const;

  /**
   * Manifest of files in the working copy whose status is not CLEAN. These are
   * also referred to as "nonnormal" files.
   */
  folly::Synchronized<std::unordered_map<RelativePath, HgStatusCode>>
      userChanges_;
  std::shared_ptr<EdenMount> edenMount_;
  std::unique_ptr<DirstatePersistence> persistence_;
};
}
}
