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
#include <folly/File.h>
#include <folly/String.h>
#include <map>
#include "eden/utils/DirType.h"
#include "eden/utils/PathFuncs.h"

namespace facebook {
namespace eden {

/** Manages the write overlay storage area.
 *
 * The overlay is where we store files that are not yet part of a snapshot.
 *
 * The contents of this storage layer are overlaid on top of the object store
 * snapshot that is active in a given mount point.
 *
 * There is one overlay area associated with each eden client instance.
 *
 * There are two important overlay concepts:
 *
 * 1. When we delete an entry from a directory that is visible in the snapshot,
 *    we need to remember that we deleted it.  We indicate that by returning
 *    dtype_t::Whiteout for those entries.
 * 2. If a directory visible in the snapshot is deleted and recreated as an
 *    empty directory, we need to ensure that the snapshot is no longer
 *    visible through the overlay.  We mark that directory as opaque.
 *
 * The methods of this class either return information about the current
 * state of the overlay, or take some action to update the state of
 * the overlay (eg: creating a file or dir).
 *
 * In the future we will move this to a persistent Trie, but we don't
 * currently have a ready-to-use Trie datastructure in folly.
 */
class Overlay {
 public:
  explicit Overlay(AbsolutePathPiece localDir);

  /** Represents the contents of a dir in the overlay.
   * The entries map may contain entries with the value dtype_t::Whiteout;
   * these indicate entries that have been deleted from the layer beneath
   * this overlay.
   */
  struct DirContents {
    /** If isOpaque, this list overrides any that might be found at the same
     * logical portion of the tree in the ObjectStore. */
    bool isOpaque{false};
    /// The list of entries, not including self and parent
    std::map<PathComponent, dtype_t> entries;
  };

  /** Returns information about the contents of a given path in the
   * overlay tree. */
  DirContents readDir(RelativePathPiece path);

  /** Delete a dir from the combined view.
   * If the directory exists in the overlay, it will be removed.
   * It is not an error if the directory does not exist in the overlay.
   *
   * If the deletion fails, an error will be thrown and the overlay
   * state will not be changed.
   *
   * if needWhiteout is true, a whiteout entry will be used to track
   * the removal.  It is only required to set needWhiteout=true if
   * path is visible in the ObjectStore.
   */
  void removeDir(RelativePathPiece path, bool needWhiteout);

  /** Create a directory in the overlay area.
   *
   * If a whiteout entry is present for any of the ancestor components
   * of path, an error will be thrown.
   *
   * If a whiteout entry is present for path, it will be removed if
   * the directory is successfully created, and the directory will be
   * marked as opaque.
   *
   * Will throw an error if the directory could not be made. */
  void makeDir(RelativePathPiece path, mode_t mode);

  /** Delete a file from the combined view.
   * Same commentary as removeDir() above, except that this operates on
   * files instead of directories.
   */
  void removeFile(RelativePathPiece path, bool needWhiteout);

  /** Open a file in the overlay area.
   * If the flags include O_CREAT, the semantics are similar to makeDir()
   * above: any ancestor component of path that is whiteout will cause
   * the creation attempt to fail, but if the path itself was marked whiteout,
   * openFile will cancel the whiteout and create the file.
   *
   * Returns a File object owning the opened file descriptor if successful,
   * throws an exception otherwise.
   */
  folly::File openFile(RelativePathPiece path, int flags, mode_t mode);

  /// Returns true if any of the path components are marked as whiteout
  bool isWhiteout(RelativePathPiece path);

  const AbsolutePath& getLocalDir() const;

 private:
  /// Computes the whiteout name for path (foo/bar -> foo/.edenm.bar)
  RelativePath computeWhiteoutName(RelativePathPiece path);
  /// Create a direct whiteout file for path
  void makeWhiteout(RelativePathPiece path);
  /// Create an opaque file in path
  void makeOpaque(RelativePathPiece path);
  /// Remove any direct whiteout marker for path
  bool removeWhiteout(RelativePathPiece path);
  /// Build out a directory tree
  void makeDirs(RelativePathPiece path);

  /// path to ".eden/CLIENT/local"
  AbsolutePath localDir_;
};
}
}
