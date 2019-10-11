/*
 * Copyright (c) Facebook, Inc. and its affiliates.
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
#include "eden/fs/inodes/InodePtr.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/utils/PathMap.h"

namespace facebook {
namespace eden {

namespace detail {

/** InodeLoader is a helper class for minimizing the number
 * of inode load calls that we need to emit when loading a list
 * of paths.
 */
class InodeLoader {
 public:
  InodeLoader() = default;

  // Arrange to load the inode for the input path
  folly::Future<InodePtr> load(RelativePathPiece path) {
    InodeLoader* parent = this;

    // Build out the tree if InodeLoaders to match the input path
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
    return parent->promises_.back().getFuture();
  }

  // Arrange to load the inode for the input path, given
  // a stringy input.  If the path is not well formed then
  // the error is recorded in the returned future.
  folly::Future<InodePtr> load(folly::StringPiece path) {
    return folly::makeFutureWith([&] { return load(RelativePathPiece(path)); });
  }

  // Called to signal that a load attempt has completed.
  // In the success case this will cause any children of
  // this inode to be loaded.
  // In the failure case this will propagate the failure to
  // any children of this node, too.
  void loaded(folly::Try<InodePtr> inodeTry) {
    for (auto& promise : promises_) {
      promise.setValue(inodeTry);
    }

    auto tree = inodeTry.hasValue() ? inodeTry->asTreePtrOrNull() : nullptr;

    for (auto& entry : children_) {
      if (inodeTry.hasException()) {
        // The attempt failed, so propagate the failure to our children
        entry.second->loaded(inodeTry);
      } else {
        // otherwise schedule the next level of lookup
        auto& childName = entry.first;
        auto& childLoader = entry.second;

        if (!tree) {
          // This inode is not a tree but we're trying to load
          // children; generate failures for these
          childLoader->loaded(folly::Try<InodePtr>(
              folly::make_exception_wrapper<std::system_error>(
                  ENOENT, std::generic_category())));
          continue;
        }

        folly::makeFutureWith([&] { return tree->getOrLoadChild(childName); })
            .thenTry(
                [loader = std::move(childLoader)](
                    folly::Try<InodePtr>&& inode) { loader->loaded(inode); });
      }
    }
  }

 private:
  // Any child nodes that we need to load.  We have to use a unique_ptr
  // for this to avoid creating a self-referential type and fail to
  // compile.  This happens to have the nice property of maintaining
  // a stable address for the contents of the InodeLoader.
  PathMap<std::unique_ptr<InodeLoader>> children_;
  // promises for the inode load attempts
  std::vector<folly::Promise<InodePtr>> promises_;

  // Helper for building out the plan during parsing
  InodeLoader* getOrCreateChild(PathComponentPiece name) {
    auto child = folly::get_ptr(children_, name);
    if (child) {
      return child->get();
    }
    auto ret = children_.emplace(name, std::make_unique<InodeLoader>());
    return ret.first->second.get();
  }
};

} // namespace detail

/** Given a `rootInode` and a list of `paths` relative to that root,
 * attempt to load the inodes.
 * The load attempt builds a tree-shaped load plan to avoid repeatedly
 * loading the same inodes over and over again.  In other words, the
 * number of inode load calls is O(number-of-unique-inodes) rather than
 * O(number-of-path-components) in the input set of paths.
 * As each matching inode is loaded, `func` is applied to it.
 * This function returns `vector<SemiFuture<Result>>` where `Result`
 * is the return type of `func`.
 * Index 0 of the results corresponds to the inode loaded for `paths[0]`,
 * and so on for each of the input paths.
 */
template <typename Func>
auto applyToInodes(
    InodePtr rootInode,
    const std::vector<std::string>& paths,
    Func func) {
  using FuncRet = folly::invoke_result_t<Func&, InodePtr&>;
  using Result = typename folly::isFutureOrSemiFuture<FuncRet>::Inner;

  detail::InodeLoader loader;

  std::vector<folly::SemiFuture<Result>> results;
  results.reserve(paths.size());
  for (const auto& path : paths) {
    results.emplace_back(loader.load(path).thenValue(
        [func](InodePtr&& inode) { return func(inode); }));
  }

  loader.loaded(folly::Try<InodePtr>(rootInode));

  return results;
}

} // namespace eden
} // namespace facebook
