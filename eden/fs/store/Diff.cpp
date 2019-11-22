/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/Diff.h"

#include <folly/Portability.h>
#include <folly/Synchronized.h>
#include <folly/futures/Future.h>
#include <folly/logging/xlog.h>
#include <memory>
#include <vector>

#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/store/DiffContext.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/store/ScmStatusDiffCallback.h"
#include "eden/fs/utils/PathFuncs.h"

using folly::Future;
using folly::makeFuture;
using folly::Try;
using folly::Unit;
using std::make_unique;
using std::vector;

namespace facebook {
namespace eden {

namespace {

struct ChildFutures {
  void add(RelativePath&& path, Future<Unit>&& future) {
    paths.emplace_back(std::move(path));
    futures.emplace_back(std::move(future));
  }

  vector<RelativePath> paths;
  vector<Future<Unit>> futures;
};

struct DiffState {
  explicit DiffState(const ObjectStore* store)
      : callback{}, context{&callback, store} {}

  ScmStatusDiffCallback callback;
  DiffContext context;
};
} // namespace

Future<Unit>
diffAddedTree(const DiffContext* context, RelativePathPiece path, Hash hash);

Future<Unit>
diffRemovedTree(const DiffContext* context, RelativePathPiece path, Hash hash);

Future<Unit> diffAddedTree(
    const DiffContext* context,
    RelativePathPiece path,
    const Tree& tree);

Future<Unit> diffRemovedTree(
    const DiffContext* context,
    RelativePathPiece path,
    const Tree& tree);

void processAddedSide(
    const DiffContext* context,
    ChildFutures& childFutures,
    RelativePathPiece parentPath,
    const TreeEntry& entry);

void processRemovedSide(
    const DiffContext* context,
    ChildFutures& childFutures,
    RelativePathPiece parentPath,
    const TreeEntry& entry);

void processBothPresent(
    const DiffContext* context,
    ChildFutures& childFutures,
    RelativePathPiece parentPath,
    const TreeEntry& entry1,
    const TreeEntry& entry2);

Future<Unit> waitOnResults(
    const DiffContext* context,
    ChildFutures&& childFutures);

/**
 * Diff two trees.
 *
 * The path argument specifies the path to these trees, and will be prefixed
 * to all differences recorded in the results.
 *
 * The differences will be recorded using a callback provided by the caller.
 */
FOLLY_NODISCARD Future<Unit> diffTrees(
    const DiffContext* context,
    RelativePathPiece path,
    const Tree& tree1,
    const Tree& tree2) {
  // A list of Futures to wait on for our children's results.
  ChildFutures childFutures;

  // Walk through the entries in both trees.
  // This relies on the fact that the entry list in each tree is always sorted.
  const auto& entries1 = tree1.getTreeEntries();
  const auto& entries2 = tree2.getTreeEntries();
  size_t idx1 = 0;
  size_t idx2 = 0;
  while (true) {
    if (idx1 >= entries1.size()) {
      if (idx2 >= entries2.size()) {
        // All Done
        break;
      }

      // This entry is present in tree2 but not tree1
      processAddedSide(context, childFutures, path, entries2[idx2]);
      ++idx2;
    } else if (idx2 >= entries2.size()) {
      // This entry is present in tree1 but not tree2
      processRemovedSide(context, childFutures, path, entries1[idx1]);
      ++idx1;
    } else if (entries1[idx1].getName() < entries2[idx2].getName()) {
      processRemovedSide(context, childFutures, path, entries1[idx1]);
      ++idx1;
    } else if (entries1[idx1].getName() > entries2[idx2].getName()) {
      processAddedSide(context, childFutures, path, entries2[idx2]);
      ++idx2;
    } else {
      processBothPresent(
          context, childFutures, path, entries1[idx1], entries2[idx2]);
      ++idx1;
      ++idx2;
    }
  }

  return waitOnResults(context, std::move(childFutures));
}

FOLLY_NODISCARD Future<Unit> diffTrees(
    const DiffContext* context,
    RelativePathPiece path,
    Hash hash1,
    Hash hash2) {
  auto treeFuture1 = context->store->getTree(hash1);
  auto treeFuture2 = context->store->getTree(hash2);
  // Optimization for the case when both tree objects are immediately ready.
  // We can avoid copying the input path in this case.
  if (treeFuture1.isReady() && treeFuture2.isReady()) {
    return diffTrees(
        context,
        path,
        *(std::move(treeFuture1).get()),
        *(std::move(treeFuture2).get()));
  }

  return folly::collect(treeFuture1, treeFuture2)
      .thenValue(
          [context, path = path.copy()](std::tuple<
                                        std::shared_ptr<const Tree>,
                                        std::shared_ptr<const Tree>>&& tup) {
            const auto& [tree1, tree2] = tup;
            return diffTrees(context, path, *tree1, *tree2);
          });
}

/**
 * Diff two commits.
 *
 * The differences will be recorded using a callback inside of DiffState and
 * will be extracted and returned to the caller.
 */
FOLLY_NODISCARD Future<Unit>
diffCommits(const DiffContext* context, Hash hash1, Hash hash2) {
  auto future1 = context->store->getTreeForCommit(hash1);
  auto future2 = context->store->getTreeForCommit(hash2);
  return collect(future1, future2)
      .thenValue([context](std::tuple<
                           std::shared_ptr<const Tree>,
                           std::shared_ptr<const Tree>>&& tup) {
        const auto& [tree1, tree2] = tup;
        return diffTrees(context, RelativePathPiece{}, *tree1, *tree2);
      });
}

FOLLY_NODISCARD Future<Unit>
diffAddedTree(const DiffContext* context, RelativePathPiece path, Hash hash) {
  auto future = context->store->getTree(hash);
  // Optimization for the case when the tree object is immediately ready.
  // We can avoid copying the input path in this case.
  if (future.isReady()) {
    return diffAddedTree(context, path, *std::move(future).get());
  }

  return std::move(future).thenValue(
      [context, path = path.copy()](std::shared_ptr<const Tree>&& tree) {
        return diffAddedTree(context, path, *tree);
      });
}

FOLLY_NODISCARD Future<Unit>
diffRemovedTree(const DiffContext* context, RelativePathPiece path, Hash hash) {
  auto future = context->store->getTree(hash);
  // Optimization for the case when the tree object is immediately ready.
  // We can avoid copying the input path in this case.
  if (future.isReady()) {
    return diffRemovedTree(context, path, *(std::move(future).get()));
  }

  return std::move(future).thenValue(
      [context, path = path.copy()](std::shared_ptr<const Tree>&& tree) {
        return diffRemovedTree(context, path, *tree);
      });
}

/**
 * Process a Tree that is present only on one side of the diff.
 */
Future<Unit> diffAddedTree(
    const DiffContext* context,
    RelativePathPiece path,
    const Tree& tree) {
  ChildFutures childFutures;
  for (const auto& childEntry : tree.getTreeEntries()) {
    processAddedSide(context, childFutures, path, childEntry);
  }
  return waitOnResults(context, std::move(childFutures));
}

/**
 * Process a Tree that is present only on one side of the diff.
 */
Future<Unit> diffRemovedTree(
    const DiffContext* context,
    RelativePathPiece path,
    const Tree& tree) {
  ChildFutures childFutures;
  for (const auto& childEntry : tree.getTreeEntries()) {
    processRemovedSide(context, childFutures, path, childEntry);
  }
  return waitOnResults(context, std::move(childFutures));
}

/**
 * Process a TreeEntry that is present only on one side of the diff.
 * We don't know yet if this TreeEntry refers to a Tree or a Blob.
 *
 * If we could not compute a result immediately we will add an entry to
 * childFutures.
 */
void processRemovedSide(
    const DiffContext* context,
    ChildFutures& childFutures,
    RelativePathPiece parentPath,
    const TreeEntry& entry) {
  if (!entry.isTree()) {
    context->callback->removedFile(parentPath + entry.getName());
    return;
  }
  auto childPath = parentPath + entry.getName();
  auto childFuture = diffRemovedTree(context, childPath, entry.getHash());
  childFutures.add(std::move(childPath), std::move(childFuture));
}

/**
 * Process a TreeEntry that is present only on one side of the diff.
 * We don't know yet if this TreeEntry refers to a Tree or a Blob.
 *
 * If we could not compute a result immediately we will add an entry to
 * childFutures.
 */
void processAddedSide(
    const DiffContext* context,
    ChildFutures& childFutures,
    RelativePathPiece parentPath,
    const TreeEntry& entry) {
  if (!entry.isTree()) {
    context->callback->addedFile(parentPath + entry.getName());
    return;
  }
  auto childPath = parentPath + entry.getName();
  auto childFuture = diffAddedTree(context, childPath, entry.getHash());
  childFutures.add(std::move(childPath), std::move(childFuture));
}

/**
 * Process TreeEntry objects that exist on both sides of the diff.
 */
void processBothPresent(
    const DiffContext* context,
    ChildFutures& childFutures,
    RelativePathPiece parentPath,
    const TreeEntry& entry1,
    const TreeEntry& entry2) {
  bool isTree1 = entry1.isTree();
  bool isTree2 = entry2.isTree();

  if (isTree1) {
    if (isTree2) {
      // tree-to-tree diff
      DCHECK_EQ(entry1.getType(), entry2.getType());
      if (entry1.getHash() == entry2.getHash()) {
        return;
      }
      auto childPath = parentPath + entry1.getName();
      auto childFuture =
          diffTrees(context, childPath, entry1.getHash(), entry2.getHash());
      childFutures.add(std::move(childPath), std::move(childFuture));
    } else {
      // tree-to-file
      // Record an ADDED entry for this path
      context->callback->addedFile(parentPath + entry1.getName());
      // Report everything in tree1 as REMOVED
      processRemovedSide(context, childFutures, parentPath, entry1);
    }
  } else {
    if (isTree2) {
      // file-to-tree
      // Add a REMOVED entry for this path
      context->callback->removedFile(parentPath + entry1.getName());

      // Report everything in tree2 as ADDED
      processAddedSide(context, childFutures, parentPath, entry2);
    } else {
      // file-to-file diff
      // Even if blobs have different hashes, they could have the same contents.
      // For example, if between the two revisions being compared, if a file was
      // changed and then later reverted. In that case, the contents would be
      // the same but the blobs would have different hashes
      // If the types are different, then this entry is definitely modified
      if (entry1.getType() != entry2.getType()) {
        context->callback->modifiedFile(parentPath + entry1.getName());
      } else {
        // If Mercurial eventually switches to using blob IDs that are solely
        // based on the file contents (as opposed to file contents + history)
        // then we could drop this extra load of the blob SHA-1, and rely only
        // on the blob ID comparison instead.
        auto compareEntryContents = folly::makeFutureWith(
            [context, path = parentPath + entry1.getName(), &entry1, &entry2] {
              auto f1 = context->store->getBlobSha1(entry1.getHash());
              auto f2 = context->store->getBlobSha1(entry2.getHash());
              return folly::collect(f1, f2).thenValue(
                  [path, context](const std::tuple<Hash, Hash>& info) {
                    const auto& [info1, info2] = info;
                    if (info1 != info2) {
                      context->callback->modifiedFile(path);
                    }
                  });
            });
        childFutures.add(
            parentPath + entry1.getName(), std::move(compareEntryContents));
      }
    }
  }
}

Future<Unit> waitOnResults(
    const DiffContext* context,
    ChildFutures&& childFutures) {
  DCHECK_EQ(childFutures.paths.size(), childFutures.futures.size());
  if (childFutures.futures.empty()) {
    return makeFuture();
  }

  return folly::collectAllSemiFuture(std::move(childFutures.futures))
      .toUnsafeFuture()
      .thenValue([context, paths = std::move(childFutures.paths)](
                     vector<Try<Unit>>&& results) {
        DCHECK_EQ(paths.size(), results.size());
        for (size_t idx = 0; idx < results.size(); ++idx) {
          const auto& result = results[idx];
          if (!result.hasException()) {
            continue;
          }
          XLOG(ERR) << "error computing SCM diff for " << paths.at(idx);
          context->callback->diffError(paths.at(idx), result.exception());
        }
      });
}

Future<std::unique_ptr<ScmStatus>>
diffCommitsForStatus(const ObjectStore* store, Hash hash1, Hash hash2) {
  return folly::makeFutureWith([&] {
    auto state = std::make_unique<DiffState>(store);
    auto statePtr = state.get();
    auto contextPtr = &(statePtr->context);
    return diffCommits(contextPtr, hash1, hash2)
        .thenValue([state = std::move(state)](auto&&) {
          return std::make_unique<ScmStatus>(state->callback.extractStatus());
        });
  });
}

Future<Unit> diffTrees(const DiffContext* context, Hash tree1, Hash tree2) {
  return folly::makeFutureWith(
      [&] { return diffTrees(context, RelativePathPiece{}, tree1, tree2); });
}

Future<Unit>
diffTrees(const DiffContext* context, const Tree& tree1, const Tree& tree2) {
  return folly::makeFutureWith(
      [&] { return diffTrees(context, RelativePathPiece{}, tree1, tree2); });
}

} // namespace eden
} // namespace facebook
