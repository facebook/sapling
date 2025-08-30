/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/Diff.h"

#include <folly/Portability.h>
#include <folly/logging/xlog.h>
#include <memory>
#include <vector>

#include "eden/common/utils/ImmediateFuture.h"
#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/store/BackingStore.h"
#include "eden/fs/store/DiffContext.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/store/ScmStatusDiffCallback.h"

using folly::Try;
using folly::Unit;
using std::make_unique;
using std::vector;

namespace facebook::eden {

/*
 * In practice, while the functions in this file are comparing two source
 * control Tree objects, they are used for comparing the current
 * (non-materialized) working directory state (as wdTree) to its corresponding
 * source control state (as scmTree).
 */
namespace {

struct TreeAndId {
  TreePtr tree;
  ObjectId id;

  static TreeAndId null() {
    return TreeAndId{nullptr, ObjectId{}};
  }
};

struct ChildFutures {
  void add(RelativePath&& path, ImmediateFuture<Unit>&& future) {
    paths.emplace_back(std::move(path));
    futures.emplace_back(std::move(future));
  }

  vector<RelativePath> paths;
  vector<ImmediateFuture<Unit>> futures;
};

/**
 * Process a TreeEntry that is present only on one side of the diff.
 * We don't know yet if this TreeEntry refers to a Tree or a Blob.
 *
 * If we could not compute a result immediately we will add an entry to
 * childFutures.
 */
void processRemovedSide(
    DiffContext* context,
    ChildFutures& childFutures,
    RelativePathPiece currentPath,
    const Tree::value_type& scmEntry) {
  auto entryPath = currentPath + scmEntry.first;
  context->callback->removedPath(
      entryPath,
      filteredEntryDtype(
          scmEntry.second.getDtype(), context->getWindowsSymlinksEnabled()));
  if (!scmEntry.second.isTree()) {
    return;
  }
  auto childFuture =
      diffRemovedTree(context, entryPath, scmEntry.second.getObjectId());
  childFutures.add(std::move(entryPath), std::move(childFuture));
}

/**
 * Process a TreeEntry that is present only on one side of the diff.
 * We don't know yet if this TreeEntry refers to a Tree or a Blob.
 *
 * If we could not compute a result immediately we will add an entry to
 * childFutures.
 */
void processAddedSide(
    DiffContext* context,
    ChildFutures& childFutures,
    RelativePathPiece currentPath,
    const Tree::value_type& wdEntry) {
  auto entryPath = currentPath + wdEntry.first;
  bool windowsSymlinksEnabled = context->getWindowsSymlinksEnabled();

  context->callback->addedPath(
      entryPath,
      filteredEntryDtype(wdEntry.second.getDtype(), windowsSymlinksEnabled));

  if (wdEntry.second.isTree()) {
    auto childFuture =
        diffAddedTree(context, entryPath, wdEntry.second.getObjectId());
    childFutures.add(std::move(entryPath), std::move(childFuture));
  }
}

/**
 * Process TreeEntry objects that exist on both sides of the diff.
 */
void processBothPresent(
    DiffContext* context,
    ChildFutures& childFutures,
    RelativePathPiece currentPath,
    const Tree::value_type& scmEntry,
    const Tree::value_type& wdEntry) {
  auto entryPath = currentPath + scmEntry.first;
  bool isTreeSCM = scmEntry.second.isTree();
  bool isTreeWD = wdEntry.second.isTree();
  bool windowsSymlinksEnabled = context->getWindowsSymlinksEnabled();

  if (isTreeSCM) {
    if (isTreeWD) {
      // tree-to-tree diff
      XDCHECK_EQ(scmEntry.second.getType(), wdEntry.second.getType());
      if (context->store->areObjectsKnownIdentical(
              scmEntry.second.getObjectId(), wdEntry.second.getObjectId())) {
        return;
      }
      context->callback->modifiedPath(entryPath, wdEntry.second.getDtype());
      auto childFuture = diffTrees(
          context,
          entryPath,
          scmEntry.second.getObjectId(),
          wdEntry.second.getObjectId());
      childFutures.add(std::move(entryPath), std::move(childFuture));
    } else {
      // tree-to-file
      // Add a ADDED entry for this path and a removal of the directory
      context->callback->addedPath(
          entryPath,
          filteredEntryDtype(
              wdEntry.second.getDtype(), windowsSymlinksEnabled));

      // Report everything in scmTree as REMOVED
      context->callback->removedPath(entryPath, scmEntry.second.getDtype());
      auto childFuture =
          diffRemovedTree(context, entryPath, scmEntry.second.getObjectId());
      childFutures.add(std::move(entryPath), std::move(childFuture));
    }
  } else {
    if (isTreeWD) {
      // file-to-tree
      // Add a REMOVED entry for this path
      context->callback->removedPath(
          entryPath,
          filteredEntryDtype(
              scmEntry.second.getDtype(), windowsSymlinksEnabled));

      // Report everything in wdEntry as ADDED
      context->callback->addedPath(entryPath, wdEntry.second.getDtype());
      auto childFuture =
          diffAddedTree(context, entryPath, wdEntry.second.getObjectId());
      childFutures.add(std::move(entryPath), std::move(childFuture));
    } else {
      // file-to-file diff
      // Even if blobs have different ids, they could have the same contents.
      // For example, if between the two revisions being compared, if a file was
      // changed and then later reverted. In that case, the contents would be
      // the same but the blobs would have different ids
      // If the types are different, then this entry is definitely modified
      if (filteredEntryType(
              scmEntry.second.getType(), windowsSymlinksEnabled) !=
          filteredEntryType(wdEntry.second.getType(), windowsSymlinksEnabled)) {
        context->callback->modifiedPath(
            entryPath,
            filteredEntryDtype(
                wdEntry.second.getDtype(), windowsSymlinksEnabled));
      } else {
        auto compareEntryContents =
            context->store
                ->areBlobsEqual(
                    scmEntry.second.getObjectId(),
                    wdEntry.second.getObjectId(),
                    context->getFetchContext())
                .thenValue([entryPath = entryPath.copy(),
                            context,
                            dtype = filteredEntryDtype(
                                scmEntry.second.getDtype(),
                                windowsSymlinksEnabled)](bool equal) {
                  if (!equal) {
                    context->callback->modifiedPath(entryPath, dtype);
                  }
                });
        childFutures.add(std::move(entryPath), std::move(compareEntryContents));
      }
    }
  }
}

FOLLY_NODISCARD ImmediateFuture<Unit> waitOnResults(
    DiffContext* context,
    ChildFutures&& childFutures) {
  XDCHECK_EQ(childFutures.paths.size(), childFutures.futures.size());
  return collectAll(std::move(childFutures.futures))
      .thenValue([context, paths = std::move(childFutures.paths)](
                     vector<Try<Unit>>&& results) {
        XDCHECK_EQ(paths.size(), results.size());
        for (size_t idx = 0; idx < results.size(); ++idx) {
          const auto& result = results[idx];
          if (!result.hasException()) {
            continue;
          }
          XLOGF(ERR, "error computing SCM diff for {}", paths.at(idx));
          context->callback->diffError(paths.at(idx), result.exception());
        }
      });
}

/**
 * Diff two trees.
 *
 * The path argument specifies the path to these trees, and will be prefixed
 * to all differences recorded in the results.
 *
 * The differences will be recorded using a callback provided by the caller.
 */
FOLLY_NODISCARD ImmediateFuture<Unit> computeTreeDiff(
    DiffContext* context,
    RelativePathPiece currentPath,
    TreePtr scmTree,
    TreePtr wdTree) {
  // A list of Futures to wait on for our children's results.
  ChildFutures childFutures;

  // Walk through the entries in both trees.
  // This relies on the fact that the entry list in each tree is always sorted.
  Tree::container emptyEntries{kPathMapDefaultCaseSensitive};
  auto scmIter = scmTree ? scmTree->cbegin() : emptyEntries.cbegin();
  auto scmEnd = scmTree ? scmTree->cend() : emptyEntries.cend();
  auto wdIter = wdTree ? wdTree->cbegin() : emptyEntries.cend();
  auto wdEnd = wdTree ? wdTree->cend() : emptyEntries.cend();
  while (true) {
    if (scmIter == scmEnd) {
      if (wdIter == wdEnd) {
        // All Done
        break;
      }
      // This entry is present in wdTree but not scmTree
      processAddedSide(context, childFutures, currentPath, *wdIter);
      ++wdIter;
    } else if (wdIter == wdEnd) {
      // This entry is present in scmTree but not wdTree
      processRemovedSide(context, childFutures, currentPath, *scmIter);
      ++scmIter;
    } else {
      auto compare = comparePathPiece(
          scmIter->first, wdIter->first, context->getCaseSensitive());
      if (compare == CompareResult::BEFORE) {
        processRemovedSide(context, childFutures, currentPath, *scmIter);
        ++scmIter;
      } else if (compare == CompareResult::AFTER) {
        processAddedSide(context, childFutures, currentPath, *wdIter);
        ++wdIter;
      } else {
        processBothPresent(
            context, childFutures, currentPath, *scmIter, *wdIter);
        ++scmIter;
        ++wdIter;
      }
    }
  }

  return waitOnResults(context, std::move(childFutures));
}

FOLLY_NODISCARD ImmediateFuture<Unit> diffTrees(
    DiffContext* context,
    RelativePathPiece currentPath,
    TreePtr scmTree,
    TreePtr wdTree) {
  if (context->isCancelled()) {
    XLOGF(
        DBG7,
        "diff() on directory {} cancelled due to client request no longer being active",
        currentPath);
    return folly::unit;
  }

  return computeTreeDiff(
      context, currentPath, std::move(scmTree), std::move(wdTree));
}

FOLLY_NODISCARD ImmediateFuture<Unit> diffTrees(
    DiffContext* context,
    RelativePathPiece currentPath,
    ImmediateFuture<TreeAndId> scmFuture,
    ImmediateFuture<TreeAndId> wdFuture) {
  auto treesFuture = collectAllSafe(std::move(scmFuture), std::move(wdFuture));

  // Optimization for the case when the trees are immediately ready. We can
  // avoid copying the input path in this case.
  auto copiedCurrentPath =
      !treesFuture.isReady() ? std::optional{currentPath.copy()} : std::nullopt;
  return std::move(treesFuture)
      .thenValue(
          [context,
           copiedCurrentPath = std::move(copiedCurrentPath),
           currentPath](std::tuple<TreeAndId, TreeAndId> tup)
              -> ImmediateFuture<folly::Unit> {
            auto [scmTree, wdTree] = std::move(tup);

            // Shortcut in the case where we're trying to diff the same tree.
            // This happens in the case in which the CLI (during eden doctor)
            // calls getScmStatusBetweenRevisions() with the same id in
            // order to check if a commit id is valid.
            if (scmTree.tree && wdTree.tree &&
                context->store->areObjectsKnownIdentical(
                    scmTree.id, wdTree.id)) {
              return folly::unit;
            }

            auto pathPiece = copiedCurrentPath.has_value()
                ? copiedCurrentPath->piece()
                : currentPath;
            return diffTrees(
                context,
                pathPiece,
                std::move(scmTree.tree),
                std::move(wdTree.tree));
          });
}

ImmediateFuture<TreeAndId> getTreeAndId(DiffContext* context, ObjectId id) {
  return context->store->getTree(id, context->getFetchContext())
      .thenValue([id](TreePtr tree) mutable {
        return TreeAndId{std::move(tree), std::move(id)};
      });
}

} // namespace

ImmediateFuture<Unit>
diffRoots(DiffContext* context, const RootId& root1, const RootId& root2) {
  auto future1 = context->store->getRootTree(root1, context->getFetchContext());
  auto future2 = context->store->getRootTree(root2, context->getFetchContext());
  return diffTrees(
      context,
      RelativePathPiece{},
      std::move(future1).thenValue([](ObjectStore::GetRootTreeResult tree) {
        return TreeAndId{tree.tree, tree.treeId};
      }),
      std::move(future2).thenValue([](ObjectStore::GetRootTreeResult tree) {
        return TreeAndId{tree.tree, tree.treeId};
      }));
}

ImmediateFuture<Unit> diffTrees(
    DiffContext* context,
    RelativePathPiece currentPath,
    ObjectId scmId,
    ObjectId wdId) {
  return diffTrees(
      context,
      currentPath,
      getTreeAndId(context, scmId),
      getTreeAndId(context, wdId));
}

ImmediateFuture<Unit> diffAddedTree(
    DiffContext* context,
    RelativePathPiece currentPath,
    ObjectId wdId) {
  return diffTrees(
      context, currentPath, TreeAndId::null(), getTreeAndId(context, wdId));
}

ImmediateFuture<Unit> diffRemovedTree(
    DiffContext* context,
    RelativePathPiece currentPath,
    ObjectId scmId) {
  return diffTrees(
      context, currentPath, getTreeAndId(context, scmId), TreeAndId::null());
}

} // namespace facebook::eden
