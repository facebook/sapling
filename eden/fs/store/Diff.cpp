/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
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
#include "eden/fs/model/git/GitIgnoreStack.h"
#include "eden/fs/store/DiffContext.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/store/ScmStatusDiffCallback.h"
#include "eden/fs/utils/Future.h"
#include "eden/fs/utils/ImmediateFuture.h"
#include "eden/fs/utils/PathFuncs.h"

using folly::Future;
using folly::makeFuture;
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
  void add(RelativePath&& path, Future<Unit>&& future) {
    paths.emplace_back(std::move(path));
    futures.emplace_back(std::move(future));
  }

  vector<RelativePath> paths;
  vector<Future<Unit>> futures;
};

static constexpr PathComponentPiece kIgnoreFilename{".gitignore"};

Future<Unit> diffAddedTree(
    DiffContext* context,
    RelativePathPiece entryPath,
    const Tree& wdTree,
    const GitIgnoreStack* ignore,
    bool isIgnored);

Future<Unit> diffRemovedTree(
    DiffContext* context,
    RelativePathPiece entryPath,
    const Tree& scmTree);

void processAddedSide(
    DiffContext* context,
    ChildFutures& childFutures,
    RelativePathPiece currentPath,
    const Tree::value_type& wdEntry,
    const GitIgnoreStack* ignore,
    bool isIgnored);

void processRemovedSide(
    DiffContext* context,
    ChildFutures& childFutures,
    RelativePathPiece currentPath,
    const Tree::value_type& scmEntry);

void processBothPresent(
    DiffContext* context,
    ChildFutures& childFutures,
    RelativePathPiece currentPath,
    const Tree::value_type& scmEntry,
    const Tree::value_type& wdEntry,
    const GitIgnoreStack* ignore,
    bool isIgnored);

Future<Unit> waitOnResults(DiffContext* context, ChildFutures&& childFutures);

/**
 * Diff two trees.
 *
 * The path argument specifies the path to these trees, and will be prefixed
 * to all differences recorded in the results.
 *
 * The differences will be recorded using a callback provided by the caller.
 */
FOLLY_NODISCARD Future<Unit> computeTreeDiff(
    DiffContext* context,
    RelativePathPiece currentPath,
    const Tree& scmTree,
    const Tree& wdTree,
    std::unique_ptr<GitIgnoreStack> ignore,
    bool isIgnored) {
  // A list of Futures to wait on for our children's results.
  ChildFutures childFutures;

  // Walk through the entries in both trees.
  // This relies on the fact that the entry list in each tree is always sorted.
  auto scmEntries = scmTree.cbegin();
  auto wdEntries = wdTree.cbegin();
  while (true) {
    if (scmEntries == scmTree.cend()) {
      if (wdEntries == wdTree.cend()) {
        // All Done
        break;
      }
      // This entry is present in wdTree but not scmTree
      processAddedSide(
          context,
          childFutures,
          currentPath,
          *wdEntries,
          ignore.get(),
          isIgnored);
      ++wdEntries;
    } else if (wdEntries == wdTree.cend()) {
      // This entry is present in scmTree but not wdTree
      processRemovedSide(context, childFutures, currentPath, *scmEntries);
      ++scmEntries;
    } else {
      auto compare = comparePathPiece(
          scmEntries->first, wdEntries->first, context->getCaseSensitive());
      if (compare == CompareResult::BEFORE) {
        processRemovedSide(context, childFutures, currentPath, *scmEntries);
        ++scmEntries;
      } else if (compare == CompareResult::AFTER) {
        processAddedSide(
            context,
            childFutures,
            currentPath,
            *wdEntries,
            ignore.get(),
            isIgnored);
        ++wdEntries;
      } else {
        processBothPresent(
            context,
            childFutures,
            currentPath,
            *scmEntries,
            *wdEntries,
            ignore.get(),
            isIgnored);
        ++scmEntries;
        ++wdEntries;
      }
    }
  }

  // Add an ensure() block that makes sure the ignore stack exists until all of
  // our children results have finished processing
  return waitOnResults(context, std::move(childFutures))
      .ensure([ignore = std::move(ignore)] {});
}

FOLLY_NODISCARD Future<Unit> loadGitIgnoreThenDiffTrees(
    PathComponentPiece gitIgnoreName,
    DiffContext* context,
    RelativePathPiece currentPath,
    const Tree& scmTree,
    const Tree& wdTree,
    const GitIgnoreStack* parentIgnore,
    bool isIgnored) {
  // TODO: load file contents directly from context->store if gitIgnoreEntry is
  // a regular file
  auto loadFileContentsFromPath = context->getLoadFileContentsFromPath();
  auto gitIgnorePath = currentPath + gitIgnoreName;
  return loadFileContentsFromPath(context->getFetchContext(), gitIgnorePath)
      .thenError(
          [entryPath = gitIgnorePath](const folly::exception_wrapper& ex) {
            // TODO: add an API to DiffCallback to report user errors like this
            // (errors that do not indicate a problem with EdenFS itself) that
            // can be returned to the caller in a thrift response
            XLOG(WARN) << "error loading gitignore at " << entryPath << ": "
                       << folly::exceptionStr(ex);
            return std::string{};
          })
      .thenValue([context,
                  currentPath = currentPath.copy(),
                  scmTree,
                  wdTree,
                  parentIgnore,
                  isIgnored](std::string&& ignoreFileContents) mutable {
        return computeTreeDiff(
            context,
            currentPath,
            scmTree,
            wdTree,
            make_unique<GitIgnoreStack>(parentIgnore, ignoreFileContents),
            isIgnored);
      });
}

FOLLY_NODISCARD Future<Unit> diffTrees(
    DiffContext* context,
    RelativePathPiece currentPath,
    const Tree& scmTree,
    const Tree& wdTree,
    const GitIgnoreStack* parentIgnore,
    bool isIgnored) {
  if (context->isCancelled()) {
    XLOG(DBG7) << "diff() on directory " << currentPath
               << " cancelled due to client request no longer being active";
    return makeFuture();
  }
  // If this directory is already ignored, we don't need to bother loading its
  // .gitignore file.  Everything inside this directory must also be ignored,
  // unless it is explicitly tracked in source control.
  //
  // Explicit include rules cannot be used to unignore files inside an ignored
  // directory.
  //
  // We check context->getLoadFileContentsFromPath() here as a way to see if we
  // are processing gitIgnore files or not, since this is only set from code
  // that enters through eden/fs/inodes/Diff.cpp. Either way, it is
  // impossible to load file contents without this set.
  if (isIgnored || !context->getLoadFileContentsFromPath()) {
    // We can pass in a null GitIgnoreStack pointer here.
    // Since the entire directory is ignored, we don't need to check ignore
    // status for any entries that aren't already tracked in source control.
    return computeTreeDiff(
        context, currentPath, scmTree, wdTree, nullptr, isIgnored);
  }

  // If this directory has a .gitignore file, load it first.
  const auto it = wdTree.find(kIgnoreFilename);
  if (it != wdTree.cend() && !it->second.isTree()) {
    return loadGitIgnoreThenDiffTrees(
        it->first,
        context,
        currentPath,
        scmTree,
        wdTree,
        parentIgnore,
        isIgnored);
  }

  return computeTreeDiff(
      context,
      currentPath,
      scmTree,
      wdTree,
      make_unique<GitIgnoreStack>(parentIgnore), // empty with no rules
      isIgnored);
}

FOLLY_NODISCARD Future<Unit> processAddedChildren(
    DiffContext* context,
    RelativePathPiece currentPath,
    const Tree& wdTree,
    std::unique_ptr<GitIgnoreStack> ignore,
    bool isIgnored) {
  ChildFutures childFutures;
  for (const auto& childEntry : wdTree) {
    processAddedSide(
        context,
        childFutures,
        currentPath,
        childEntry,
        ignore.get(),
        isIgnored);
  }

  // Add an ensure() block that makes sure the ignore stack exists until all of
  // our children results have finished processing
  return waitOnResults(context, std::move(childFutures))
      .ensure([ignore = std::move(ignore)] {});
}

FOLLY_NODISCARD Future<Unit> loadGitIgnoreThenProcessAddedChildren(
    PathComponentPiece gitIgnoreName,
    DiffContext* context,
    RelativePathPiece currentPath,
    const Tree& wdTree,
    const GitIgnoreStack* parentIgnore,
    bool isIgnored) {
  auto loadFileContentsFromPath = context->getLoadFileContentsFromPath();
  auto gitIgnorePath = currentPath + gitIgnoreName;
  return loadFileContentsFromPath(context->getFetchContext(), gitIgnorePath)
      .thenError(
          [entryPath = gitIgnorePath](const folly::exception_wrapper& ex) {
            XLOG(WARN) << "error loading gitignore at " << entryPath << ": "
                       << folly::exceptionStr(ex);
            return std::string{};
          })
      .thenValue([context,
                  currentPath = currentPath.copy(),
                  wdTree,
                  parentIgnore,
                  isIgnored](std::string&& ignoreFileContents) mutable {
        return processAddedChildren(
            context,
            currentPath,
            wdTree,
            make_unique<GitIgnoreStack>(parentIgnore, ignoreFileContents),
            isIgnored);
      });
}

/**
 * Process a Tree that is present only on one side of the diff.
 */
FOLLY_NODISCARD Future<Unit> diffAddedTree(
    DiffContext* context,
    RelativePathPiece currentPath,
    const Tree& wdTree,
    const GitIgnoreStack* parentIgnore,
    bool isIgnored) {
  if (context->isCancelled()) {
    XLOG(DBG7) << "diff() on directory " << currentPath
               << " cancelled due to client request no longer being active";
    return makeFuture();
  }
  ChildFutures childFutures;

  // If this directory is already ignored, we don't need to bother loading its
  // .gitignore file.  Everything inside this directory must also be ignored,
  // unless it is explicitly tracked in source control.
  //
  // Also, if we are not honoring gitignored files, then do not bother loading
  // its .gitignore file
  //
  // Explicit include rules cannot be used to unignore files inside an ignored
  // directory.
  //
  // We check context->getLoadFileContentsFromPath() here as a way to see if we
  // are processing gitIgnore files or not, since this is only set from code
  // that enters through eden/fs/inodes/DiffTree.cpp. Either way, it is
  // impossible to load file contents without this set.
  if (isIgnored || !context->getLoadFileContentsFromPath()) {
    // We can pass in a null GitIgnoreStack pointer here.
    // Since the entire directory is ignored, we don't need to check ignore
    // status for any entries that aren't already tracked in source control.
    return processAddedChildren(
        context, currentPath, wdTree, nullptr, isIgnored);
  }

  // If this directory has a .gitignore file, load it first.
  const auto it = wdTree.find(kIgnoreFilename);
  if (it != wdTree.cend() && !it->second.isTree()) {
    return loadGitIgnoreThenProcessAddedChildren(
        it->first, context, currentPath, wdTree, parentIgnore, isIgnored);
  }

  return processAddedChildren(
      context,
      currentPath,
      wdTree,
      make_unique<GitIgnoreStack>(parentIgnore), // empty with no rules
      isIgnored);
}

/**
 * Process a Tree that is present only on one side of the diff.
 */
FOLLY_NODISCARD Future<Unit> diffRemovedTree(
    DiffContext* context,
    RelativePathPiece currentPath,
    const Tree& scmTree) {
  if (context->isCancelled()) {
    XLOG(DBG7) << "diff() on directory " << currentPath
               << " cancelled due to client request no longer being active";
    return makeFuture();
  }
  ChildFutures childFutures;
  for (const auto& childEntry : scmTree) {
    processRemovedSide(context, childFutures, currentPath, childEntry);
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
    DiffContext* context,
    ChildFutures& childFutures,
    RelativePathPiece currentPath,
    const Tree::value_type& scmEntry) {
  context->callback->removedPath(
      currentPath + scmEntry.first, scmEntry.second.getDType());
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
    context->callback->addedPath(entryPath, wdEntry.second.getDType());
  } else if (context->listIgnored) {
    context->callback->ignoredPath(entryPath, wdEntry.second.getDType());
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
      if (scmEntry.second.getHash() == wdEntry.second.getHash()) {
        return;
      }
      context->callback->modifiedPath(entryPath, wdEntry.second.getDType());
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
          context->callback->ignoredPath(entryPath, wdEntry.second.getDType());
        }
      } else {
        context->callback->addedPath(entryPath, wdEntry.second.getDType());
      }

      // Report everything in scmTree as REMOVED
      context->callback->removedPath(entryPath, scmEntry.second.getDType());
      auto childFuture =
          diffRemovedTree(context, entryPath, scmEntry.second.getHash());
      childFutures.add(std::move(entryPath), std::move(childFuture));
    }
  } else {
    if (isTreeWD) {
      // file-to-tree
      // Add a REMOVED entry for this path
      context->callback->removedPath(entryPath, scmEntry.second.getDType());

      // Report everything in wdEntry as ADDED
      context->callback->addedPath(entryPath, wdEntry.second.getDType());
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
        context->callback->modifiedPath(entryPath, wdEntry.second.getDType());
      } else {
        // If Mercurial eventually switches to using blob IDs that are solely
        // based on the file contents (as opposed to file contents + history)
        // then we could drop this extra load of the blob SHA-1, and rely only
        // on the blob ID comparison instead.
        auto compareEntryContents =
            folly::makeFutureWith([context,
                                   entryPath = currentPath + scmEntry.first,
                                   &scmEntry,
                                   &wdEntry] {
              auto scmFuture = context->store->getBlobSha1(
                  scmEntry.second.getHash(), context->getFetchContext());
              auto wdFuture = context->store->getBlobSha1(
                  wdEntry.second.getHash(), context->getFetchContext());
              return collectAllSafe(scmFuture, wdFuture)
                  .thenValue([entryPath = entryPath.copy(),
                              context,
                              dtype = scmEntry.second.getDType()](
                                 const std::tuple<Hash20, Hash20>& info) {
                    const auto& [scmHash, wdHash] = info;
                    if (scmHash != wdHash) {
                      context->callback->modifiedPath(entryPath, dtype);
                    }
                  })
                  .semi()
                  .via(&folly::QueuedImmediateExecutor::instance());
            });
        childFutures.add(std::move(entryPath), std::move(compareEntryContents));
      }
    }
  }
}

FOLLY_NODISCARD Future<Unit> waitOnResults(
    DiffContext* context,
    ChildFutures&& childFutures) {
  XDCHECK_EQ(childFutures.paths.size(), childFutures.futures.size());
  if (childFutures.futures.empty()) {
    return makeFuture();
  }

  return folly::collectAll(std::move(childFutures.futures))
      .toUnsafeFuture()
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

} // namespace

Future<Unit>
diffRoots(DiffContext* context, const RootId& root1, const RootId& root2) {
  auto future1 = context->store->getRootTree(root1, context->getFetchContext());
  auto future2 = context->store->getRootTree(root2, context->getFetchContext());
  return collectAllSafe(future1, future2)
      .semi()
      .via(&folly::QueuedImmediateExecutor::instance())
      .thenValue([context](std::tuple<
                           std::shared_ptr<const Tree>,
                           std::shared_ptr<const Tree>>&& tup) {
        const auto& [tree1, tree2] = tup;

        // This happens in the case in which the CLI (during eden doctor) calls
        // getScmStatusBetweenRevisions() with the same hash in order to check
        // if a commit hash is valid.
        if (tree1->getHash() == tree2->getHash()) {
          return makeFuture();
        }

        return diffTrees(
            context, RelativePathPiece{}, *tree1, *tree2, nullptr, false);
      });
}

FOLLY_NODISCARD Future<Unit> diffTrees(
    DiffContext* context,
    RelativePathPiece currentPath,
    ObjectId scmHash,
    ObjectId wdHash,
    const GitIgnoreStack* ignore,
    bool isIgnored) {
  auto treesFuture = collectAllSafe(
      context->store->getTree(scmHash, context->getFetchContext()),
      context->store->getTree(wdHash, context->getFetchContext()));

  // Optimization for the case when the trees are immediately ready. We can
  // avoid copying the input path in this case.
  auto copiedCurrentPath =
      !treesFuture.isReady() ? std::optional{currentPath.copy()} : std::nullopt;
  return std::move(treesFuture)
      .thenValue([context,
                  copiedCurrentPath = std::move(copiedCurrentPath),
                  currentPath,
                  ignore,
                  isIgnored](std::tuple<
                             std::shared_ptr<const Tree>,
                             std::shared_ptr<const Tree>>&& tup) {
        const auto& [scmTree, wdTree] = tup;
        auto pathPiece = copiedCurrentPath.has_value()
            ? copiedCurrentPath->piece()
            : currentPath;
        return diffTrees(
                   context, pathPiece, *scmTree, *wdTree, ignore, isIgnored)
            .semi();
      })
      .semi()
      .via(&folly::QueuedImmediateExecutor::instance());
}

FOLLY_NODISCARD Future<Unit> diffAddedTree(
    DiffContext* context,
    RelativePathPiece currentPath,
    ObjectId wdHash,
    const GitIgnoreStack* ignore,
    bool isIgnored) {
  auto wdFuture = context->store->getTree(wdHash, context->getFetchContext());

  // Optimization for the case when the tree object is immediately ready. We
  // can avoid copying the input path in this case.
  auto copiedCurrentPath =
      !wdFuture.isReady() ? std::optional{currentPath.copy()} : std::nullopt;
  return std::move(wdFuture)
      .thenValue([context,
                  copiedCurrentPath = std::move(copiedCurrentPath),
                  currentPath,
                  ignore,
                  isIgnored](std::shared_ptr<const Tree>&& wdTree) {
        auto pathPiece = copiedCurrentPath.has_value()
            ? copiedCurrentPath->piece()
            : currentPath;
        return diffAddedTree(context, pathPiece, *wdTree, ignore, isIgnored)
            .semi();
      })
      .semi()
      .via(&folly::QueuedImmediateExecutor::instance());
}

FOLLY_NODISCARD Future<Unit> diffRemovedTree(
    DiffContext* context,
    RelativePathPiece currentPath,
    ObjectId scmHash) {
  auto scmFuture = context->store->getTree(scmHash, context->getFetchContext());

  // Optimization for the case when the tree object is immediately ready. We
  // can avoid copying the input path in this case.
  auto copiedCurrentPath =
      !scmFuture.isReady() ? std::optional{currentPath.copy()} : std::nullopt;
  return std::move(scmFuture)
      .thenValue([context,
                  copiedCurrentPath = std::move(copiedCurrentPath),
                  currentPath](std::shared_ptr<const Tree>&& tree) {
        auto pathPiece = copiedCurrentPath.has_value()
            ? copiedCurrentPath->piece()
            : currentPath;
        return diffRemovedTree(context, pathPiece, *tree).semi();
      })
      .semi()
      .via(&folly::QueuedImmediateExecutor::instance());
}

} // namespace facebook::eden
