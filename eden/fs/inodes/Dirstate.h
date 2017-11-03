/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once
#include <folly/Optional.h>
#include <folly/Synchronized.h>
#include <folly/experimental/StringKeyedUnorderedMap.h>
#include "eden/fs/inodes/DirstatePersistence.h"
#include "eden/fs/inodes/InodePtrFwd.h"
#include "eden/fs/inodes/gen-cpp2/hgdirstate_types.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/service/gen-cpp2/EdenService.h"
#include "eden/fs/utils/PathFuncs.h"

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
} // namespace fusell

/**
 * Returns the single-char representation of the status used by `hg status`.
 * Note that this differs from the corresponding entry in the _VALUES_TO_NAMES
 * map for a Thrift enum.
 */
char hgStatusCodeChar(StatusCode code);

struct DirstateAddRemoveError {
  DirstateAddRemoveError(RelativePathPiece p, folly::StringPiece s)
      : path{p}, errorMessage{s.str()} {}

  RelativePath path;
  std::string errorMessage;
};
inline bool operator==(
    const DirstateAddRemoveError& lhs,
    const DirstateAddRemoveError& rhs) {
  return lhs.path == rhs.path && lhs.errorMessage == rhs.errorMessage;
}
inline bool operator!=(
    const DirstateAddRemoveError& lhs,
    const DirstateAddRemoveError& rhs) {
  return !(lhs == rhs);
}
std::ostream& operator<<(std::ostream& os, const DirstateAddRemoveError& error);

std::ostream& operator<<(std::ostream& os, const ThriftHgStatus& status);

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

  /**
   * Get the status information about files that are changed.
   *
   * This is used for implementing "hg status".  Returns the data as a thrift
   * structure that can be returned to the eden hg extension.
   *
   * @param listIgnored Whether or not to report information about ignored
   *     files.
   */
  ThriftHgStatus getStatus(bool listIgnored) const;

  /**
   * Clean up the Dirstate after the current commit has changed.
   *
   * This removes Add and Remove directives if the corresponding files have
   * been added or removed in the new source control state.
   */
  folly::Future<folly::Unit> onSnapshotChanged(const Tree* rootTree);

  /** Clears out the data structures associated with this Dirstate. */
  void clear();

  void createBackup(PathComponent backupName);
  void restoreBackup(PathComponent backupName);

  hgdirstate::DirstateTuple hgGetDirstateTuple(
      const RelativePathPiece filename);
  void hgSetDirstateTuple(
      const RelativePathPiece filename,
      const hgdirstate::DirstateTuple* tuple);
  bool hgDeleteDirstateTuple(const RelativePathPiece filename);

  std::unordered_map<RelativePath, hgdirstate::DirstateTuple>
  hgGetNonnormalFiles() const;

  void hgCopyMapPut(
      const RelativePathPiece dest,
      const RelativePathPiece source);
  RelativePath hgCopyMapGet(const RelativePathPiece dest) const;
  folly::StringKeyedUnorderedMap<RelativePath> hgCopyMapGetAll() const;

 private:
  /**
   * If `filename` exists in the manifest as a file (not a directory), returns
   * the mode of the file as recorded in the manifest.
   */
  folly::Optional<mode_t> isInManifestAsFile(
      const RelativePathPiece filename) const;

  /**
   * @return the path to use with createBackup() and restoreBackup().
   */
  AbsolutePath createBackupPath(PathComponent backupName);

  /** The EdenMount object that owns this Dirstate */
  EdenMount* const mount_{nullptr};
  DirstatePersistence persistence_;

  folly::Synchronized<DirstateData> data_;
};
} // namespace eden
} // namespace facebook
