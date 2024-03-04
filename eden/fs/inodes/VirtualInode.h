/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <sys/stat.h>

#include "eden/common/utils/ImmediateFuture.h"
#include "eden/common/utils/RefPtr.h"
#include "eden/fs/inodes/InodePtr.h"
#include "eden/fs/inodes/UnmaterializedUnloadedBlobDirEntry.h"
#include "eden/fs/model/BlobMetadata.h"
#include "eden/fs/model/EntryAttributeFlags.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/model/TreeFwd.h"

namespace facebook::eden {

class ObjectStore;
class ObjectFetchContext;
using ObjectFetchContextPtr = RefPtr<ObjectFetchContext>;

namespace detail {
using VariantVirtualInode = std::
    variant<InodePtr, UnmaterializedUnloadedBlobDirEntry, TreePtr, TreeEntry>;
} // namespace detail

/**
 * VirtualInode allows read-only queries over a mount independent of the state
 * it's in. If a mount has loaded inodes, they will be queried. Otherwise,
 * source control objects will be fetched from the BackingStore, avoiding
 * needing to query the overlay and track loaded inodes.
 *
 * Note that "virtual" in VirtualInode refers to the fact that these objects are
 * inode-like, but may not reference an inode under the hood. They are unrelated
 * to the BSD vnode concept.
 */
class VirtualInode {
 public:
  explicit VirtualInode(InodePtr value) : variant_(std::move(value)) {}

  explicit VirtualInode(UnmaterializedUnloadedBlobDirEntry value)
      : variant_(std::move(value)) {}

  explicit VirtualInode(TreePtr value, mode_t mode)
      : variant_(std::move(value)), treeMode_(mode) {}

  explicit VirtualInode(TreeEntry value) : variant_(std::move(value)) {
    XCHECK(!value.isTree())
        << "TreeEntries which represent a tree should be resolved to a tree "
        << "before being constructed into VirtualInode";
  }

  /**
   * Returns the contained InodePtr.
   *
   * If there is not one, throws a std::exception.
   */
  InodePtr asInodePtr() const;

  dtype_t getDtype() const;

  bool isDirectory() const;

  std::optional<ObjectId> getObjectId() const;

  /**
   * Discover the contained data type.
   *
   * These functions should not be used outside of unit tests.
   * VirtualInode should "transparently" look like a file or directory to
   * most users.
   */
  enum class ContainedType {
    Inode,
    DirEntry, // aka UnmaterializedUnloadedBlobDirEntry
    Tree,
    TreeEntry,
  };
  ContainedType testGetContainedType() const;

  /**
   * Returns nullopt if the entry has a non source control file type.
   * Source Control types are currently limited to symlinks, executable files,
   * regular files and directories. So something like a FIFO or a socket would
   * fall into nullopt here.
   */
  ImmediateFuture<std::optional<TreeEntryType>> getTreeEntryType(
      RelativePathPiece path,
      const ObjectFetchContextPtr& fetchContext,
      bool windowsSymlinksEnabled) const;

  /**
   * Get the VirtualInode object for a child of this directory.
   *
   * Unlike TreeInode::getOrLoadChild, this method avoids loading the child's
   * inode if it is not already loaded, instead falling back to looking up the
   * object in the ObjectStore.
   */
  ImmediateFuture<VirtualInode> getOrFindChild(
      PathComponentPiece childName,
      RelativePathPiece path,
      const std::shared_ptr<ObjectStore>& objectStore,
      const ObjectFetchContextPtr& fetchContext) const;

  ImmediateFuture<Hash20> getSHA1(
      RelativePathPiece path,
      const std::shared_ptr<ObjectStore>& objectStore,
      const ObjectFetchContextPtr& fetchContext) const;

  ImmediateFuture<Hash32> getBlake3(
      RelativePathPiece path,
      const std::shared_ptr<ObjectStore>& objectStore,
      const ObjectFetchContextPtr& fetchContext) const;

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
      EntryAttributeFlags requestedAttributes,
      RelativePathPiece path,
      const std::shared_ptr<ObjectStore>& objectStore,
      const ObjectFetchContextPtr& fetchContext) const;

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
      const std::shared_ptr<ObjectStore>& objectStore,
      const ObjectFetchContextPtr& fetchContext) const;

  /**
   * Retrieves VirtualInode for each of the children of this
   * directory.
   *
   * fetchContext is used in the returned ImmediateFutures, it must have a
   * lifetime longer than these futures.
   */
  folly::Try<
      std::vector<std::pair<PathComponent, ImmediateFuture<VirtualInode>>>>
  getChildren(
      RelativePathPiece path,
      const std::shared_ptr<ObjectStore>& objectStore,
      const ObjectFetchContextPtr& fetchContext);

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
      EntryAttributeFlags requestedAttributes,
      RelativePath path,
      const std::shared_ptr<ObjectStore>& objectStore,
      const ObjectFetchContextPtr& fetchContext);

 private:
  /**
   * Helper function for getChildrenAttributes
   */
  ImmediateFuture<BlobMetadata> getBlobMetadata(
      RelativePathPiece path,
      const std::shared_ptr<ObjectStore>& objectStore,
      const ObjectFetchContextPtr& fetchContext,
      bool blake3Required = false) const;

  EntryAttributes getEntryAttributesForNonFile(
      EntryAttributeFlags requestedAttributes,
      RelativePathPiece path,
      std::optional<TreeEntryType> entryType,
      int errorCode,
      std::string additionalErrorContext = {}) const;

  /**
   * The main object this encapsulates
   */
  detail::VariantVirtualInode variant_;

  /**
   * The mode_t iff this contains a Tree
   *
   * The Tree's TreeEntry tells us about the mode_t of a tree, it must be saved
   * here for return in the stat() call.
   */
  mode_t treeMode_{0};
};

} // namespace facebook::eden
