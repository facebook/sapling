/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/Diff.h"

#include <folly/Portability.h>
#include <folly/Synchronized.h>
#include <folly/logging/xlog.h>
#include <memory>
#include <vector>

#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/model/git/GitIgnoreStack.h"
#include "eden/fs/store/BackingStore.h"
#include "eden/fs/store/DiffContext.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/store/ScmStatusDiffCallback.h"
#include "eden/fs/utils/ImmediateFuture.h"
#include "eden/fs/utils/PathFuncs.h"

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

struct ChildFutures {
  void add(RelativePath&& path, ImmediateFuture<Unit>&& future) {
    paths.emplace_back(std::move(path));
    futures.emplace_back(std::move(future));
  }

  vector<RelativePath> paths;
  vector<ImmediateFuture<Unit>> futures;
};

static constexpr PathComponentPiece kIgnoreFilename{".gitignore"};

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
  context->callback->removedPath(
      currentPath + scmEntry.first, scmEntry.second.getDtype());
  if (!scmEntry.second.isTree()) {
    return;
  }
  auto entryPath = currentPath + scmEntry.first;
  auto childFuture =
      diffRemovedTree(context, entryPath, scmEntry.second.getHash());
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
    const Tree::value_type& wdEntry,
    const GitIgnoreStack* ignore,
    bool isIgnored) {
  bool entryIgnored = isIgnored;
  auto entryPath = currentPath + wdEntry.first;
  if (!isIgnored && ignore) {
    auto fileType =
        wdEntry.second.isTree() ? GitIgnore::TYPE_DIR : GitIgnore::TYPE_FILE;
    auto ignoreStatus = ignore->match(entryPath, fileType);
    if (ignoreStatus == GitIgnore::HIDDEN) {
      // Completely skip over hidden entries.
      // This is used for reserved directories like .hg and .eden
      return;
    }
    entryIgnored = (ignoreStatus == GitIgnore::EXCLUDE);
  }

  if (!entryIgnored) {
    context->callback->addedPath(entryPath, wdEntry.second.getDtype());
  } else if (context->listIgnored) {
    context->callback->ignoredPath(entryPath, wdEntry.second.getDtype());
  } else {
    // Don't bother reporting this ignored file since
    // listIgnored is false.
  }

  if (wdEntry.second.isTree()) {
    if (!entryIgnored || context->listIgnored) {
      auto childFuture = diffAddedTree(
          context, entryPath, wdEntry.second.getHash(), ignore, entryIgnored);
      childFutures.add(std::move(entryPath), std::move(childFuture));
    }
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
    const Tree::value_type& wdEntry,
    const GitIgnoreStack* ignore,
    bool isIgnored) {
  bool entryIgnored = isIgnored;
  auto entryPath = currentPath + scmEntry.first;
  bool isTreeSCM = scmEntry.second.isTree();
  bool isTreeWD = wdEntry.second.isTree();

  // If wdEntry and scmEntry are both files (or symlinks) then we don't need
  // to bother computing the ignore status: the file is explicitly tracked in
  // source control, so we should report it's status even if it would normally
  // be ignored.
  if (!isIgnored && (isTreeWD || isTreeSCM) && ignore) {
    auto fileType = isTreeWD ? GitIgnore::TYPE_DIR : GitIgnore::TYPE_FILE;
    auto ignoreStatus = ignore->match(entryPath, fileType);
    if (ignoreStatus == GitIgnore::HIDDEN) {
      // This is rather unexpected.  We don't expect to find entries in
      // source control using reserved hidden names.
      // Treat this as ignored for now.
      entryIgnored = true;
    } else if (ignoreStatus == GitIgnore::EXCLUDE) {
      entryIgnored = true;
    } else {
      entryIgnored = false;
    }
  }

  if (isTreeSCM) {
    if (isTreeWD) {
      // tree-to-tree diff
      XDCHECK_EQ(scmEntry.second.getType(), wdEntry.second.getType());
      if (context->store->areObjectsKnownIdentical(
              scmEntry.second.getHash(), wdEntry.second.getHash())) {
        return;
      }
      context->callback->modifiedPath(entryPath, wdEntry.second.getDtype());
      auto childFuture = diffTrees(
          context,
          entryPath,
          scmEntry.second.getHash(),
          wdEntry.second.getHash(),
          ignore,
          entryIgnored);
      childFutures.add(std::move(entryPath), std::move(childFuture));
    } else {
      // tree-to-file
      // Add a ADDED entry for this path and a removal of the directory
      if (entryIgnored) {
        if (context->listIgnored) {
          context->callback->ignoredPath(entryPath, wdEntry.second.getDtype());
        }
      } else {
        context->callback->addedPath(entryPath, wdEntry.second.getDtype());
      }

      // Report everything in scmTree as REMOVED
      context->callback->removedPath(entryPath, scmEntry.second.getDtype());
      auto childFuture =
          diffRemovedTree(context, entryPath, scmEntry.second.getHash());
      childFutures.add(std::move(entryPath), std::move(childFuture));
    }
  } else {
    if (isTreeWD) {
      // file-to-tree
      // Add a REMOVED entry for this path
      context->callback->removedPath(entryPath, scmEntry.second.getDtype());

      // Report everything in wdEntry as ADDED
      context->callback->addedPath(entryPath, wdEntry.second.getDtype());
      auto childFuture = diffAddedTree(
          context, entryPath, wdEntry.second.getHash(), ignore, entryIgnored);
      childFutures.add(std::move(entryPath), std::move(childFuture));
    } else {
      // file-to-file diff
      // Even if blobs have different hashes, they could have the same contents.
      // For example, if between the two revisions being compared, if a file was
      // changed and then later reverted. In that case, the contents would be
      // the same but the blobs would have different hashes
      // If the types are different, then this entry is definitely modified
      if (scmEntry.second.getType() != wdEntry.second.getType()) {
        context->callback->modifiedPath(entryPath, wdEntry.second.getDtype());
      } else {
        auto compareEntryContents =
            context->store
                ->areBlobsEqual(
                    scmEntry.second.getHash(),
                    wdEntry.second.getHash(),
                    context->getFetchContext())
                .thenValue([entryPath = entryPath.copy(),
                            context,
                            dtype = scmEntry.second.getDtype()](bool equal) {
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
          XLOG(ERR) << "error computing SCM diff for " << paths.at(idx);
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
    std::shared_ptr<const Tree> scmTree,
    std::shared_ptr<const Tree> wdTree,
    std::unique_ptr<GitIgnoreStack> ignore,
    bool isIgnored) {
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
      processAddedSide(
          context, childFutures, currentPath, *wdIter, ignore.get(), isIgnored);
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
        processAddedSide(
            context,
            childFutures,
            currentPath,
            *wdIter,
            ignore.get(),
            isIgnored);
        ++wdIter;
      } else {
        processBothPresent(
            context,
            childFutures,
            currentPath,
            *scmIter,
            *wdIter,
            ignore.get(),
            isIgnored);
        ++scmIter;
        ++wdIter;
      }
    }
  }

  // Add an ensure() block that makes sure the ignore stack exists until all of
  // our children results have finished processing
  return waitOnResults(context, std::move(childFutures))
      .ensure([ignore = std::move(ignore)] {});
}

/**
 * Load the content of the .gitignore file and return it.
 */
ImmediateFuture<std::string> loadGitIgnore(
    DiffContext* context,
    const TreeEntry& treeEntry,
    RelativePath gitIgnorePath) {
  auto type = treeEntry.getType();
  if (type != TreeEntryType::REGULAR_FILE &&
      type != TreeEntryType::EXECUTABLE_FILE) {
    XLOG(WARN) << "error loading gitignore at " << gitIgnorePath
               << ": not a regular file";
    return std::string{};
  } else {
    const auto& hash = treeEntry.getHash();
    return context->store->getBlob(hash, context->getFetchContext())
        .thenTry([entryPath = std::move(gitIgnorePath)](
                     folly::Try<std::shared_ptr<const Blob>> blobTry) {
          if (blobTry.hasException()) {
            // TODO: add an API to DiffCallback to report user
            // errors like this (errors that do not indicate a
            // problem with EdenFS itself) that can be returned to
            // the caller in a thrift response
            XLOG(WARN) << "error loading gitignore at " << entryPath << ": "
                       << folly::exceptionStr(blobTry.exception());

            return std::string{};
          }
          const auto& contentsBuf = blobTry.value()->getContents();
          folly::io::Cursor cursor(&contentsBuf);
          return cursor.readFixedString(contentsBuf.computeChainDataLength());
        });
  }
}

FOLLY_NODISCARD ImmediateFuture<Unit> diffTrees(
    DiffContext* context,
    RelativePathPiece currentPath,
    std::shared_ptr<const Tree> scmTree,
    std::shared_ptr<const Tree> wdTree,
    const GitIgnoreStack* parentIgnore,
    bool isIgnored) {
  if (context->isCancelled()) {
    XLOG(DBG7) << "diff() on directory " << currentPath
               << " cancelled due to client request no longer being active";
    return folly::unit;
  }
  // If this directory is already ignored, we don't need to bother loading its
  // .gitignore file.  Everything inside this directory must also be ignored,
  // unless it is explicitly tracked in source control.
  //
  // Explicit include rules cannot be used to unignore files inside an ignored
  // directory.
  if (isIgnored) {
    // We can pass in a null GitIgnoreStack pointer here.
    // Since the entire directory is ignored, we don't need to check ignore
    // status for any entries that aren't already tracked in source control.
    return computeTreeDiff(
        context,
        currentPath,
        std::move(scmTree),
        std::move(wdTree),
        nullptr,
        isIgnored);
  }

  ImmediateFuture<std::string> gitIgnore{std::in_place};
  if (wdTree) {
    // If this directory has a .gitignore file, load it first.
    const auto it = wdTree->find(kIgnoreFilename);
    if (it != wdTree->cend() && !it->second.isTree()) {
      gitIgnore = loadGitIgnore(context, it->second, currentPath + it->first);
    }
  }

  return std::move(gitIgnore).thenValue(
      [context,
       currentPath = currentPath.copy(),
       scmTree = std::move(scmTree),
       wdTree = std::move(wdTree),
       parentIgnore,
       isIgnored](std::string gitIgnore) mutable {
        auto gitIgnoreStack =
            std::make_unique<GitIgnoreStack>(parentIgnore, gitIgnore);
        return computeTreeDiff(
            context,
            currentPath,
            std::move(scmTree),
            std::move(wdTree),
            std::move(gitIgnoreStack),
            isIgnored);
      });
}

FOLLY_NODISCARD ImmediateFuture<Unit> diffTrees(
    DiffContext* context,
    RelativePathPiece currentPath,
    ImmediateFuture<std::shared_ptr<const Tree>> scmFuture,
    ImmediateFuture<std::shared_ptr<const Tree>> wdFuture,
    const GitIgnoreStack* ignore,
    bool isIgnored) {
  auto treesFuture = collectAllSafe(std::move(scmFuture), std::move(wdFuture));

  // Optimization for the case when the trees are immediately ready. We can
  // avoid copying the input path in this case.
  auto copiedCurrentPath =
      !treesFuture.isReady() ? std::optional{currentPath.copy()} : std::nullopt;
  return std::move(treesFuture)
      .thenValue(
          [context,
           copiedCurrentPath = std::move(copiedCurrentPath),
           currentPath,
           ignore,
           isIgnored](std::tuple<
                      std::shared_ptr<const Tree>,
                      std::shared_ptr<const Tree>> tup)
              -> ImmediateFuture<folly::Unit> {
            auto [scmTree, wdTree] = std::move(tup);

            // Shortcut in the case where we're trying to diff the same tree.
            // This happens in the case in which the CLI (during eden doctor)
            // calls getScmStatusBetweenRevisions() with the same hash in
            // order to check if a commit hash is valid.
            if (scmTree && wdTree &&
                context->store->areObjectsKnownIdentical(
                    scmTree->getHash(), wdTree->getHash())) {
              return folly::unit;
            }

            auto pathPiece = copiedCurrentPath.has_value()
                ? copiedCurrentPath->piece()
                : currentPath;
            return diffTrees(
                context,
                pathPiece,
                std::move(scmTree),
                std::move(wdTree),
                ignore,
                isIgnored);
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
      std::move(future1),
      std::move(future2),
      nullptr,
      false);
}

ImmediateFuture<Unit> diffTrees(
    DiffContext* context,
    RelativePathPiece currentPath,
    ObjectId scmHash,
    ObjectId wdHash,
    const GitIgnoreStack* ignore,
    bool isIgnored) {
  return diffTrees(
      context,
      currentPath,
      context->store->getTree(scmHash, context->getFetchContext()),
      context->store->getTree(wdHash, context->getFetchContext()),
      ignore,
      isIgnored);
}

ImmediateFuture<Unit> diffAddedTree(
    DiffContext* context,
    RelativePathPiece currentPath,
    ObjectId wdHash,
    const GitIgnoreStack* ignore,
    bool isIgnored) {
  return diffTrees(
      context,
      currentPath,
      std::shared_ptr<const Tree>{nullptr},
      context->store->getTree(wdHash, context->getFetchContext()),
      ignore,
      isIgnored);
}

ImmediateFuture<Unit> diffRemovedTree(
    DiffContext* context,
    RelativePathPiece currentPath,
    ObjectId scmHash) {
  return diffTrees(
      context,
      currentPath,
      context->store->getTree(scmHash, context->getFetchContext()),
      std::shared_ptr<const Tree>{nullptr},
      nullptr,
      false);
}

} // namespace facebook::eden
