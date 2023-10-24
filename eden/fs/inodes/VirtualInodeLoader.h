/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include <folly/ExceptionWrapper.h>
#include <folly/FBVector.h>
#include <folly/MapUtil.h>
#include <folly/functional/Invoke.h>
#include <folly/futures/Future.h>
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/inodes/VirtualInode.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/utils/CaseSensitivity.h"
#include "eden/fs/utils/PathMap.h"

namespace facebook::eden {

namespace detail {

/** InodeLoader is a helper class for minimizing the number
 * of inode load calls that we need to emit when loading a list
 * of paths.
 */
class VirtualInodeLoader {
 public:
  VirtualInodeLoader() = default;

  // Arrange to load the inode for the input path
  folly::SemiFuture<VirtualInode> load(RelativePathPiece path) {
    VirtualInodeLoader* parent = this;

    // Build out the tree if VirtualInodeLoaders to match the input path
    for (auto name : path.components()) {
      auto child = parent->getOrCreateChild(name);
      parent = child;
    }

    // Whichever node we finished on is the last component
    // of the input path and thus is one for which we need to
    // request info.
    // Note that parent can potentially == this if the input path
    // is the root.

    parent->promises_.emplace_back();
    return parent->promises_.back().getSemiFuture();
  }

  // Called to signal that a load attempt has completed.
  // In the success case this will cause any children of
  // this inode to be loaded.
  // In the failure case this will propagate the failure to
  // any children of this node, too.
  ImmediateFuture<folly::Unit> loaded(
      folly::Try<VirtualInode> inodeTreeTry,
      RelativePathPiece path,
      const std::shared_ptr<ObjectStore>& store,
      const ObjectFetchContextPtr& fetchContext) {
    for (auto& promise : promises_) {
      promise.setValue(inodeTreeTry);
    }

    auto isTree = inodeTreeTry.hasValue() ? inodeTreeTry->isDirectory() : false;

    std::vector<ImmediateFuture<folly::Unit>> futures;
    futures.reserve(children_.size());
    for (auto& entry : children_) {
      auto& childName = entry.first;
      auto& childLoader = entry.second;
      auto childPath = path + childName;

      if (inodeTreeTry.hasException()) {
        // The attempt failed, so propagate the failure to our children
        futures.push_back(
            childLoader->loaded(inodeTreeTry, childPath, store, fetchContext));
      } else if (!isTree) {
        // This inode is not a tree but we're trying to load
        // children; generate failures for these
        futures.push_back(childLoader->loaded(
            folly::Try<VirtualInode>(
                folly::make_exception_wrapper<std::system_error>(
                    ENOENT, std::generic_category())),
            childPath,
            store,
            fetchContext));
      } else {
        futures.push_back(
            makeImmediateFutureWith([&] {
              return inodeTreeTry.value().getOrFindChild(
                  childName, childPath, store, fetchContext);
            })
                .thenTry([loader = std::move(childLoader),
                          childPath,
                          store,
                          fetchContext = fetchContext.copy()](
                             folly::Try<VirtualInode>&& childInodeTreeTry) {
                  return loader->loaded(
                      std::move(childInodeTreeTry),
                      childPath,
                      store,
                      fetchContext);
                }));
      }
    }

    return collectAllSafe(std::move(futures)).unit();
  }

 private:
  // Any child nodes that we need to load.  We have to use a unique_ptr
  // for this to avoid creating a self-referential type and fail to
  // compile.  This happens to have the nice property of maintaining
  // a stable address for the contents of the VirtualInodeLoader.
  PathMap<std::unique_ptr<VirtualInodeLoader>> children_{
      CaseSensitivity::Sensitive};
  // promises for the inode load attempts
  std::vector<folly::Promise<VirtualInode>> promises_;

  // Helper for building out the plan during parsing
  VirtualInodeLoader* getOrCreateChild(PathComponentPiece name) {
    auto child = folly::get_ptr(children_, name);
    if (child) {
      return child->get();
    }
    auto ret = children_.emplace(name, std::make_unique<VirtualInodeLoader>());
    return ret.first->second.get();
  }
};

} // namespace detail

/** Given a `rootInode` and a list of `paths` relative to that root,
 * attempt to load the VirtualInode for each.
 *
 * The load attempt builds a tree-shaped load plan to avoid repeatedly
 * loading the same objects over and over again.  In other words, the
 * number of inode load calls is O(number-of-unique-objects) rather than
 * O(number-of-path-components) in the input set of paths.
 * As each matching object is loaded, `func` is applied to it.
 * This function returns `vector<SemiFuture<Result>>` where `Result`
 * is the return type of `func`.
 * Index 0 of the results corresponds to the inode loaded for `paths[0]`,
 * and so on for each of the input paths.
 *
 * Note: The `paths` are supplied as std::string because they are inputs from a
 * Thrift call. They are converted by the `load(std::string)` overload above in
 * order to ensure that if a path is invalid, the results include an exception
 * entry for that path, as the caller expects 1:1 numbers of records in/out.
 */
template <typename Func>
auto applyToVirtualInode(
    InodePtr rootInode,
    const std::vector<std::string>& paths,
    Func func,
    const std::shared_ptr<ObjectStore>& store,
    const ObjectFetchContextPtr& fetchContext) {
  using FuncRet = folly::invoke_result_t<Func&, VirtualInode, RelativePath>;
  using Result = typename folly::isFutureOrSemiFuture<FuncRet>::Inner;

  detail::VirtualInodeLoader loader;

  // Func may not be copyable, so wrap it in a shared_ptr.
  auto cb = std::make_shared<Func>(std::move(func));

  std::vector<folly::SemiFuture<Result>> results;
  results.reserve(paths.size());
  for (const auto& path : paths) {
    auto result = folly::makeSemiFutureWith([&] {
      auto relPath = RelativePathPiece{path};
      return loader.load(relPath).deferValue(
          [cb, relPath = relPath.copy()](VirtualInode&& inode) mutable {
            return (*cb)(std::move(inode), std::move(relPath));
          });
    });
    results.push_back(std::move(result));
  }

  return loader
      .loaded(
          folly::Try<VirtualInode>(VirtualInode{std::move(rootInode)}),
          RelativePath(),
          store,
          fetchContext)
      .thenValue([results = std::move(results)](auto&&) mutable {
        return folly::collectAll(std::move(results));
      });
}

} // namespace facebook::eden
