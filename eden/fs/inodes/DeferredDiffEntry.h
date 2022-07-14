/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <memory>
#include "eden/fs/inodes/InodePtrFwd.h"
#include "eden/fs/utils/DirType.h"
#include "eden/fs/utils/PathFuncs.h"

namespace folly {
struct Unit;
} // namespace folly

namespace facebook::eden {

template <typename T>
class ImmediateFuture;

class DiffContext;
class GitIgnoreStack;
class ObjectId;
class Hash20;
class ObjectStore;
class TreeEntry;
class TreeInode;
class DiffCallback;

/**
 * A helper class for use in TreeInode::diff()
 *
 * While diff() holds the contents_ lock it computes a set of child entries
 * that need to be examined later once it releases the contents_ lock.
 * DeferredDiffEntry is used to store the data about which children need to be
 * examined.  The DeferredDiffEntry subclasses contain the logic for how to
 * then perform the diff on the child entry.
 */
class DeferredDiffEntry {
 public:
  explicit DeferredDiffEntry(DiffContext* context, RelativePath&& path)
      : context_{context}, path_{std::move(path)} {}
  virtual ~DeferredDiffEntry() {}

  const RelativePath& getPath() const {
    return path_;
  }

  FOLLY_NODISCARD virtual ImmediateFuture<folly::Unit> run() = 0;

  static std::unique_ptr<DeferredDiffEntry> createUntrackedEntry(
      DiffContext* context,
      RelativePath path,
      ImmediateFuture<InodePtr>&& inodeFuture,
      const GitIgnoreStack* ignore,
      bool isIgnored);

  static std::unique_ptr<DeferredDiffEntry> createRemovedEntry(
      DiffContext* context,
      RelativePath path,
      const TreeEntry& scmEntry);

  static std::unique_ptr<DeferredDiffEntry> createModifiedEntry(
      DiffContext* context,
      RelativePath path,
      const TreeEntry& scmEntry,
      ObjectId currentBlobHash,
      dtype_t currentDType);

  static std::unique_ptr<DeferredDiffEntry> createModifiedEntry(
      DiffContext* context,
      RelativePath path,
      std::vector<TreeEntry> scmEntries,
      ImmediateFuture<InodePtr>&& inodeFuture,
      const GitIgnoreStack* ignore,
      bool isIgnored);

  static std::unique_ptr<DeferredDiffEntry> createModifiedScmEntry(
      DiffContext* context,
      RelativePath path,
      ObjectId scmHash,
      ObjectId wdHash,
      const GitIgnoreStack* ignore,
      bool isIgnored);

  static std::unique_ptr<DeferredDiffEntry> createAddedScmEntry(
      DiffContext* context,
      RelativePath path,
      ObjectId wdHash,
      const GitIgnoreStack* ignore,
      bool isIgnored);

  static std::unique_ptr<DeferredDiffEntry> createRemovedScmEntry(
      DiffContext* context,
      RelativePath path,
      ObjectId scmHash);

 protected:
  DiffContext* const context_;
  RelativePath const path_;
};
} // namespace facebook::eden
