/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <sys/stat.h>
#include <variant>

#include <folly/String.h>

#include "eden/fs/inodes/InodePtr.h"
#include "eden/fs/inodes/UnmaterializedUnloadedBlobDirEntry.h"
#include "eden/fs/model/BlobMetadata.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/utils/ImmediateFuture.h"

namespace facebook::eden {

class ObjectStore;
class ObjectFetchContext;

namespace detail {
using TreePtr = std::shared_ptr<const Tree>;

using VariantInodeOrTreeOrEntry = std::
    variant<InodePtr, UnmaterializedUnloadedBlobDirEntry, TreePtr, TreeEntry>;
} // namespace detail

class InodeOrTreeOrEntry {
 public:
  explicit InodeOrTreeOrEntry(InodePtr& value) : variant_(value) {}
  explicit InodeOrTreeOrEntry(InodePtr&& value) : variant_(std::move(value)) {}

  explicit InodeOrTreeOrEntry(UnmaterializedUnloadedBlobDirEntry&& value)
      : variant_(std::move(value)) {}

  explicit InodeOrTreeOrEntry(const detail::TreePtr& value, mode_t mode)
      : variant_(value), treeMode_(mode) {}
  explicit InodeOrTreeOrEntry(detail::TreePtr&& value, mode_t mode)
      : variant_(std::move(value)), treeMode_(mode) {}

  explicit InodeOrTreeOrEntry(const TreeEntry& value) : variant_(value) {
    XCHECK(!value.isTree())
        << "TreeEntries which represent a tree should be resolved to a tree "
        << "before being constructed into InodeOrTreeOrEntry";
  }
  explicit InodeOrTreeOrEntry(TreeEntry&& value) : variant_(std::move(value)) {
    XCHECK(!value.isTree())
        << "TreeEntries which represent a tree should be resolved to a tree "
        << "before being constructed into InodeOrTreeOrEntry";
  }

  /**
   * Returns the contained InodePtr.
   *
   * If there is not one, throws a std::exception.
   */
  InodePtr asInodePtr() const;

  dtype_t getDtype() const;

  bool isDirectory() const;

  /**
   * Discover the contained data type.
   *
   * These functions should not be used outside of unit tests.
   * InodeOrTreeOrEntry should "transparently" look like a file or directory to
   * most users.
   */
  enum class ContainedType {
    Inode,
    DirEntry, // aka UnmaterializedUnloadedBlobDirEntry
    Tree,
    TreeEntry,
  };
  ContainedType testGetContainedType() const;

  ImmediateFuture<TreeEntryType> getTreeEntryType(
      RelativePathPiece path,
      ObjectFetchContext& fetchContext) const;

  /**
   * Get the InodeOrTreeOrEntry object for a child of this directory.
   *
   * Unlike TreeInode::getOrLoadChild, this method avoids loading the child's
   * inode if it is not already loaded, instead falling back to looking up the
   * object in the ObjectStore.
   */
  ImmediateFuture<InodeOrTreeOrEntry> getOrFindChild(
      PathComponentPiece childName,
      RelativePathPiece path,
      ObjectStore* objectStore,
      ObjectFetchContext& fetchContext) const;

  ImmediateFuture<Hash20> getSHA1(
      RelativePathPiece path,
      ObjectStore* objectStore,
      ObjectFetchContext& fetchContext) const;

  /**
   * Get all the available attributes for a file entry in this tree. Available
   * attributes are currently:
   * - sha1
   * - size
   * - source control type
   *
   * Note that we return error values for sha1s and sizes of directories and
   * symlinks.
   */
  ImmediateFuture<EntryAttributes> getEntryAttributes(
      RelativePathPiece path,
      ObjectStore* objectStore,
      ObjectFetchContext& fetchContext) const;

  /**
   * Emulate stat in a way that works for source control.
   *
   * Will just run stat on the Inode if possible, otherwise returns a stat
   * structure with the st_mode and st_size data from the
   * ObjectStore/DirEntry/TreeEntry, and the st_mtim set to the passed in
   * lastCheckoutTime.
   */
  ImmediateFuture<struct stat> stat(
      const struct timespec& lastCheckoutTime,
      ObjectStore* objectStore,
      ObjectFetchContext& fetchContext) const;

  /**
   * Retrieves InodeOrTreeOrEntry for each of the children of this
   * directory.
   *
   * fetchContext is used in the returned ImmediateFutures, it must have a
   * lifetime longer than these futures.
   */
  folly::Try<std::vector<
      std::pair<PathComponent, ImmediateFuture<InodeOrTreeOrEntry>>>>
  getChildren(
      RelativePathPiece path,
      ObjectStore* objectStore,
      ObjectFetchContext& fetchContext);

  /**
   * Collect all available attributes for all of the children
   * of a directory. All available attributes are currently:
   * - sha1
   * - size
   * - source control type
   * Note that we return error values for sha1s and sizes of directories and
   * symlinks.
   */
  ImmediateFuture<
      std::vector<std::pair<PathComponent, folly::Try<EntryAttributes>>>>
  getChildrenAttributes(
      RelativePath path,
      ObjectStore* objectStore,
      ObjectFetchContext& fetchContext);

 private:
  /**
   * Helper function for getOrFindChild when the current node is a Tree.
   */
  static ImmediateFuture<InodeOrTreeOrEntry> getOrFindChild(
      detail::TreePtr tree,
      PathComponentPiece childName,
      RelativePathPiece path,
      ObjectStore* objectStore,
      ObjectFetchContext& fetchContext);

  /**
   * The main object this encapsulates
   */
  detail::VariantInodeOrTreeOrEntry variant_;

  /**
   * The mode_t iff this contains a Tree
   *
   * The Tree's TreeEntry tells us about the mode_t of a tree, it must be saved
   * here for return in the stat() call.
   */
  mode_t treeMode_{0};
};

} // namespace facebook::eden
