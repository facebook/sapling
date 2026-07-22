/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/GlobNode.h"
#include <eden/fs/inodes/InodePtrFwd.h>
#include <iomanip>
#include <iostream>
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/telemetry/TaskTrace.h"

using folly::StringPiece;

namespace facebook::eden {

namespace {

// Policy objects to help avoid duplicating the core globbing logic.
// We can walk over two different kinds of trees; either TreeInodes
// or raw Trees from the storage layer.  While they have similar
// properties, accessing them is a little different.  These policy
// objects are thin shims that make access more uniform.

/** TreeInodePtrRoot wraps a TreeInodePtr for globbing.
 * TreeInodes require that a lock be held while its entries
 * are iterated.
 * We only need to prefetch children of TreeInodes that are
 * not materialized.
 */
struct TreeInodePtrRoot {
  TreeInodePtr root;

  explicit TreeInodePtrRoot(TreeInodePtr root) : root(std::move(root)) {}

  /** Return an object that holds a lock over the children */
  folly::Synchronized<TreeInodeState>::ConstLockedPtr lockContents() {
    return root->lockContentsRead();
  }

  /** Given the return value from lockContents and a name,
   * return a pointer to the child with that name, or nullptr
   * if there is no match */
  template <typename CONTENTS>
  typename DirContents::const_pointer FOLLY_NULLABLE
  lookupEntry(CONTENTS& contents, PathComponentPiece name) {
    auto it = contents->entries.find(name);
    if (it != contents->entries.end()) {
      return &*it;
    }
    return nullptr;
  }

  /** Return an object that can be used in a generic for()
   * constructor to iterate over the contents.  You must supply
   * the CONTENTS object you obtained via lockContents().
   * The returned iterator yields ENTRY elements that can be
   * used with the entryXXX methods below. */
  const DirContents& iterate(
      const folly::Synchronized<TreeInodeState>::ConstLockedPtr& contents)
      const {
    return contents->entries;
  }

  /** Arrange to load a child TreeInode */
  folly::coro::now_task<TreeInodePtr> co_getOrLoadChildTree(
      PathComponentPiece name,
      const ObjectFetchContextPtr& context) {
    co_return co_await root->co_getOrLoadChildTree(name, context);
  }
  /** Returns true if we should call co_getOrLoadChildTree() for the given
   * ENTRY.  We only do this if the child is already materialized */
  bool entryShouldLoadChildTree(const DirEntry* entry) {
    return entry->isMaterialized();
  }

  /** Returns true if the given entry is a tree */
  bool entryIsTree(const DirEntry* entry) {
    return entry->isDirectory();
  }

  /** Returns true if the given entry is restricted */
  bool entryIsRestricted(const DirEntry* entry) {
    return entry->isRestricted();
  }

  /** Returns true if we should prefetch the blob content for the entry.
   * We only do this if the child is not already materialized */
  bool entryShouldPrefetch(const DirEntry* entry) {
    return !entry->isMaterialized() && !entryIsTree(entry);
  }
};
} // namespace

folly::coro::now_task<folly::Unit> GlobNode::evaluate(
    std::shared_ptr<ObjectStore> store,
    const ObjectFetchContextPtr& context,
    RelativePathPiece rootPath,
    TreeInodePtr root,
    PrefetchList* fileBlobsToPrefetch,
    ResultList* globResult,
    const RootId& originRootId) const {
  co_return co_await evaluateImpl<TreeInodePtrRoot, TreeInodePtr>(
      store.get(),
      context,
      rootPath,
      TreeInodePtrRoot(std::move(root)),
      fileBlobsToPrefetch,
      globResult,
      originRootId);
}

} // namespace facebook::eden
