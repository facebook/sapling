/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <variant>

#include <folly/String.h>

#include "eden/fs/inodes/InodePtr.h"
#include "eden/fs/inodes/UnmaterializedUnloadedBlobDirEntry.h"
#include "eden/fs/model/BlobMetadata.h"
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

  explicit InodeOrTreeOrEntry(const detail::TreePtr& value) : variant_(value) {}
  explicit InodeOrTreeOrEntry(detail::TreePtr&& value)
      : variant_(std::move(value)) {}

  explicit InodeOrTreeOrEntry(const TreeEntry& value) : variant_(value) {}
  explicit InodeOrTreeOrEntry(TreeEntry&& value) : variant_(std::move(value)) {}

  /**
   * Returns the contained InodePtr.
   *
   * If there is not one, throws a std::exception.
   */
  InodePtr asInodePtr() const;

  dtype_t getDtype() const;

  bool isDirectory() const {
    return getDtype() == dtype_t::Dir;
  }

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
      ObjectFetchContext& fetchContext);

  ImmediateFuture<Hash20> getSHA1(
      RelativePathPiece path,
      ObjectStore* objectStore,
      ObjectFetchContext& fetchContext) const;

  ImmediateFuture<BlobMetadata> getBlobMetadata(
      RelativePathPiece path,
      ObjectStore* objectStore,
      ObjectFetchContext& fetchContext) const;

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

  detail::VariantInodeOrTreeOrEntry variant_;
};

} // namespace facebook::eden
