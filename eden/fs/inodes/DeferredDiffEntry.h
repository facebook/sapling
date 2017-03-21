/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <memory>
#include "eden/fs/inodes/InodePtrFwd.h"
#include "eden/utils/PathFuncs.h"

namespace folly {
template <typename T>
class Future;
class Unit;
}

namespace facebook {
namespace eden {

class Hash;
class ObjectStore;
class TreeEntry;
class TreeInode;
class InodeDiffCallback;

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
  explicit DeferredDiffEntry(RelativePath&& path) : path_{std::move(path)} {}
  virtual ~DeferredDiffEntry() {}

  const RelativePath& getPath() const {
    return path_;
  }

  virtual folly::Future<folly::Unit> run(InodeDiffCallback* callback) = 0;

  static std::unique_ptr<DeferredDiffEntry> createUntrackedEntry(
      RelativePath path,
      InodePtr inode,
      bool isIgnored,
      bool listIgnored);

  /*
   * This is named differently from the createUntrackedEntry() function above
   * just to avoid ambiguous overload calls--folly::Future<X> can unfortunately
   * be implicitly constructed from X.  We could help the compiler avoid the
   * ambiguity by making the Future<InodePtr> version of createUntrackedEntry()
   * be a template method.  However, just using a separate name is easier for
   * now.
   */
  static std::unique_ptr<DeferredDiffEntry> createUntrackedEntryFromInodeFuture(
      RelativePath path,
      folly::Future<InodePtr>&& inodeFuture,
      bool isIgnored,
      bool listIgnored);

  static std::unique_ptr<DeferredDiffEntry> createRemovedEntry(
      RelativePath path,
      ObjectStore* store,
      const TreeEntry& scmEntry);

  static std::unique_ptr<DeferredDiffEntry> createModifiedEntry(
      RelativePath path,
      const TreeEntry& scmEntry,
      InodePtr inode,
      bool isIgnored,
      bool listIgnored);

  static std::unique_ptr<DeferredDiffEntry> createModifiedEntry(
      RelativePath path,
      ObjectStore* store,
      const TreeEntry& scmEntry,
      folly::Future<InodePtr>&& inodeFuture,
      bool isIgnored,
      bool listIgnored);

  static std::unique_ptr<DeferredDiffEntry> createModifiedEntry(
      RelativePath path,
      ObjectStore* store,
      const TreeEntry& scmEntry,
      Hash currentBlobHash);

 private:
  RelativePath path_;
};
}
}
