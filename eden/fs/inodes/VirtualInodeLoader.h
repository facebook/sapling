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
#include <folly/coro/Collect.h>
#include <folly/coro/Invoke.h>
#include <folly/coro/safe/NowTask.h>
#include <folly/functional/Invoke.h>
#include <folly/futures/Future.h>

#include <folly/coro/ViaIfAsync.h>
#include "eden/common/utils/CaseSensitivity.h"
#include "eden/common/utils/PathMap.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/inodes/VirtualInode.h"
#include "eden/fs/store/ObjectFetchContext.h"

namespace facebook::eden {

namespace detail {

/** InodeLoader is a helper class for minimizing the number
 * of inode load calls that we need to emit when loading a list
 * of paths.
 */
class VirtualInodeLoader {
 public:
  VirtualInodeLoader() = default;

  // DEPRECATED: use co_load() directly. Kept only because
  // applyToVirtualInode still consumes SemiFuture chains;
  // delete once that path is migrated to coroutines.
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

  // Arrange to load the inode for the input path (coroutine interface).
  // Returns a pointer to this node so the caller can read the result
  // after co_loaded() completes.
  VirtualInodeLoader* co_load(RelativePathPiece path) {
    VirtualInodeLoader* parent = this;
    for (auto name : path.components()) {
      parent = parent->getOrCreateChild(name);
    }
    return parent;
  }

  // DEPRECATED: use co_loaded() directly. Kept only because
  // applyToVirtualInode still consumes ImmediateFuture chains;
  // delete once that path is migrated to coroutines.
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

  // Coroutine-native version of loaded(). Recursively resolves all children
  // via co_getOrFindChild, storing results directly in each node. Children
  // are resolved in parallel via collectAllRange, matching the concurrency
  // of the futures-based loaded() which uses collectAllSafe.
  folly::coro::now_task<void> co_loaded(
      folly::Try<VirtualInode> inodeTry,
      RelativePathPiece path,
      const std::shared_ptr<ObjectStore>& store,
      const ObjectFetchContextPtr& fetchContext) {
    result_ = inodeTry;

    auto isTree = inodeTry.hasValue() ? inodeTry->isDirectory() : false;

    std::vector<folly::coro::Task<void>> childTasks;
    childTasks.reserve(children_.size());
    for (auto& [childName, childLoader] : children_) {
      childTasks.push_back(
          folly::coro::co_invoke(
              [loader = childLoader.get(),
               &name = childName,
               childPath = path + childName,
               &inodeTry,
               isTree,
               store,
               fetchContext =
                   fetchContext.copy()]() -> folly::coro::Task<void> {
                if (inodeTry.hasException()) {
                  co_await loader->co_loaded(
                      inodeTry, childPath, store, fetchContext);
                } else if (!isTree) {
                  co_await loader->co_loaded(
                      folly::Try<VirtualInode>(
                          folly::make_exception_wrapper<std::system_error>(
                              ENOENT, std::generic_category())),
                      childPath,
                      store,
                      fetchContext);
                } else {
                  auto childInodeTry = co_await folly::coro::co_awaitTry(
                      inodeTry.value().co_getOrFindChild(
                          name, childPath, store, fetchContext));
                  co_await loader->co_loaded(
                      std::move(childInodeTry), childPath, store, fetchContext);
                }
              }));
    }
    co_await folly::coro::collectAllRange(std::move(childTasks));
  }

  // Access the resolved result after co_loaded() completes.
  const folly::Try<VirtualInode>& result() const {
    return result_;
  }

 private:
  // Any child nodes that we need to load.  We have to use a unique_ptr
  // for this to avoid creating a self-referential type and fail to
  // compile.  This happens to have the nice property of maintaining
  // a stable address for the contents of the VirtualInodeLoader.
  PathMap<std::unique_ptr<VirtualInodeLoader>> children_{
      CaseSensitivity::Sensitive};
  // promises for the inode load attempts (futures interface)
  std::vector<folly::Promise<VirtualInode>> promises_;
  // resolved result (coroutine interface, populated by co_loaded)
  folly::Try<VirtualInode> result_;

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

// Result type for co_applyToVirtualInode: extracts the inner type from Func's
// co_awaitable return type (SemiFuture<T>, now_task<T>, etc.).
// Uses semi_await_result_t so non-awaitable return types produce a compile
// error.
template <typename Func>
using VirtualInodeResult = folly::coro::semi_await_result_t<
    folly::invoke_result_t<Func&, VirtualInode, RelativePath>>;

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
  // DEPRECATED: use co_applyToVirtualInode directly. Kept only because
  // EdenServiceHandler and VirtualInodeLoaderTest still consume
  // ImmediateFuture chains; delete once those paths are migrated to coroutines.
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

/**
 * Coroutine-native version of applyToVirtualInode. Uses VirtualInodeLoader's
 * co_load/co_loaded interface for tree-shaped path resolution via
 * co_getOrFindChild — no promises, SemiFutures, or .semi() bridges. func may
 * return anything co_awaitable (SemiFuture<T>, now_task<T>, etc.).
 *
 * Both inode resolution and func application are parallelized: resolution
 * via the loader's tree-shaped plan, func via collectAllRange.
 *
 * CONTRACT: The result of invoking func is immediately co_awaited within the
 * same expression via co_awaitTry. This is critical when func returns
 * now_task<T>: now_task requires immediate awaiting to guarantee reference
 * safety for captured references. Do not store, move, or defer the result of
 * func.
 */
template <typename Func>
folly::coro::now_task<std::vector<folly::Try<VirtualInodeResult<Func>>>>
co_applyToVirtualInode(
    InodePtr rootInode,
    const std::vector<std::string>& paths,
    Func func,
    const std::shared_ptr<ObjectStore>& store,
    const ObjectFetchContextPtr& fetchContext) {
  using Result = VirtualInodeResult<Func>;

  detail::VirtualInodeLoader loader;

  // Func may not be copyable, so wrap it in a shared_ptr.
  auto cb = std::make_shared<Func>(std::move(func));

  // Build the load plan. Each entry is either a pointer to a loader node
  // (for valid paths) or an exception (for invalid paths).
  struct PathEntry {
    detail::VirtualInodeLoader* node{nullptr};
    RelativePath path;
    folly::exception_wrapper error;
  };
  std::vector<PathEntry> entries;
  entries.reserve(paths.size());
  for (const auto& path : paths) {
    PathEntry entry;
    try {
      auto relPath = RelativePathPiece{path};
      entry.node = loader.co_load(relPath);
      entry.path = relPath.copy();
    } catch (const std::exception&) {
      entry.error = folly::exception_wrapper(std::current_exception());
    }
    entries.push_back(std::move(entry));
  }

  // Resolve all inodes via the coroutine loader.
  co_await loader.co_loaded(
      folly::Try<VirtualInode>(VirtualInode{std::move(rootInode)}),
      RelativePath(),
      store,
      fetchContext);

  // Apply func to each resolved inode in parallel.
  std::vector<folly::coro::Task<folly::Try<Result>>> tasks;
  tasks.reserve(entries.size());
  for (auto& entry : entries) {
    tasks.push_back(
        folly::coro::co_invoke(
            [cb,
             node = entry.node,
             path = std::move(entry.path),
             error = std::move(entry.error)]() mutable
                -> folly::coro::Task<folly::Try<Result>> {
              if (error) {
                co_return folly::Try<Result>(std::move(error));
              }
              auto& inodeTry = node->result();
              if (inodeTry.hasException()) {
                co_return folly::Try<Result>(inodeTry.exception());
              }
              co_return co_await folly::coro::co_awaitTry(
                  (*cb)(inodeTry.value(), std::move(path)));
            }));
  }

  co_return co_await folly::coro::collectAllRange(std::move(tasks));
}

} // namespace facebook::eden
