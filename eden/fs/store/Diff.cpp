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
#include "eden/fs/store/DiffCallback.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/store/ScmStatusDiffCallback.h"
#include "eden/fs/utils/PathFuncs.h"

using folly::Future;
using folly::makeFuture;
using folly::Synchronized;
using folly::Try;
using folly::Unit;
using std::make_unique;
using std::vector;

namespace facebook {
namespace eden {

namespace {

/**
 * TreeDiffer knows how to diff source control Tree objects.
 */
class TreeDiffer {
 public:
  explicit TreeDiffer(ObjectStore* store, DiffCallback* callback)
      : store_(store), callback_(callback) {}

  /**
   * Diff two commits.
   *
   * The differences will be recorded using a callback inside of DiffState and
   * will be extracted and returned to the caller.
   */
  FOLLY_NODISCARD Future<Unit> diffCommits(Hash hash1, Hash hash2);

  /**
   * Diff two trees.
   *
   * The path argument specifies the path to these trees, and will be prefixed
   * to all differences recorded in the results.
   *
   * The differences will be recorded using a callback provided by the caller.
   */
  FOLLY_NODISCARD Future<Unit>
  diffTrees(RelativePathPiece path, Hash hash1, Hash hash2);
  FOLLY_NODISCARD Future<Unit>
  diffTrees(RelativePathPiece path, const Tree& tree1, const Tree& tree2);

 private:
  struct ChildFutures {
    void add(RelativePath&& path, Future<Unit>&& future) {
      paths.emplace_back(std::move(path));
      futures.emplace_back(std::move(future));
    }

    vector<RelativePath> paths;
    vector<Future<Unit>> futures;
  };

  Future<Unit> diffAddedTree(RelativePathPiece path, Hash hash);
  Future<Unit> diffRemovedTree(RelativePathPiece path, Hash hash);

  FOLLY_NODISCARD Future<Unit> diffAddedTree(
      RelativePathPiece path,
      const Tree& tree);
  FOLLY_NODISCARD Future<Unit> diffRemovedTree(
      RelativePathPiece path,
      const Tree& tree);

  void processAddedSide(
      ChildFutures& futures,
      RelativePathPiece parentPath,
      const TreeEntry& entry);
  void processRemovedSide(
      ChildFutures& futures,
      RelativePathPiece parentPath,
      const TreeEntry& entry);
  void processBothPresent(
      ChildFutures& futures,
      RelativePathPiece parentPath,
      const TreeEntry& entry1,
      const TreeEntry& entry2);

  Future<Unit> waitOnResults(ChildFutures&& childFutures);

  ObjectStore* store_;
  DiffCallback* callback_;
};

Future<Unit> TreeDiffer::diffCommits(Hash hash1, Hash hash2) {
  auto future1 = store_->getTreeForCommit(hash1);
  auto future2 = store_->getTreeForCommit(hash2);
  return collect(future1, future2)
      .thenValue([this](std::tuple<
                        std::shared_ptr<const Tree>,
                        std::shared_ptr<const Tree>>&& tup) {
        auto tree1 = std::get<0>(tup);
        auto tree2 = std::get<1>(tup);
        return diffTrees(RelativePathPiece{}, *tree1, *tree2);
      });
}

Future<Unit>
TreeDiffer::diffTrees(RelativePathPiece path, Hash hash1, Hash hash2) {
  auto treeFuture1 = store_->getTree(hash1);
  auto treeFuture2 = store_->getTree(hash2);
  // Optimization for the case when both tree objects are immediately ready.
  // We can avoid copying the input path in this case.
  if (treeFuture1.isReady() && treeFuture2.isReady()) {
    return diffTrees(
        path, *std::move(treeFuture1).get(), *std::move(treeFuture2).get());
  }

  return folly::collect(treeFuture1, treeFuture2)
      .thenValue(
          [this, path = path.copy()](std::tuple<
                                     std::shared_ptr<const Tree>,
                                     std::shared_ptr<const Tree>>&& tup) {
            auto tree1 = std::get<0>(tup);
            auto tree2 = std::get<1>(tup);
            return diffTrees(path, *tree1, *tree2);
          });
}

Future<Unit> TreeDiffer::diffTrees(
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
      processAddedSide(childFutures, path, entries2[idx2]);
      ++idx2;
    } else if (idx2 >= entries2.size()) {
      // This entry is present in tree1 but not tree2
      processRemovedSide(childFutures, path, entries1[idx1]);
      ++idx1;
    } else if (entries1[idx1].getName() < entries2[idx2].getName()) {
      processRemovedSide(childFutures, path, entries1[idx1]);
      ++idx1;
    } else if (entries1[idx1].getName() > entries2[idx2].getName()) {
      processAddedSide(childFutures, path, entries2[idx2]);
      ++idx2;
    } else {
      processBothPresent(childFutures, path, entries1[idx1], entries2[idx2]);
      ++idx1;
      ++idx2;
    }
  }

  return waitOnResults(std::move(childFutures));
}

Future<Unit> TreeDiffer::diffAddedTree(RelativePathPiece path, Hash hash) {
  auto future = store_->getTree(hash);
  // Optimization for the case when the tree object is immediately ready.
  // We can avoid copying the input path in this case.
  if (future.isReady()) {
    return diffAddedTree(path, *std::move(future).get());
  }

  return std::move(future).thenValue(
      [this, path = path.copy()](std::shared_ptr<const Tree>&& tree) {
        return diffAddedTree(path, *tree);
      });
}

Future<Unit> TreeDiffer::diffRemovedTree(RelativePathPiece path, Hash hash) {
  auto future = store_->getTree(hash);
  // Optimization for the case when the tree object is immediately ready.
  // We can avoid copying the input path in this case.
  if (future.isReady()) {
    return diffRemovedTree(path, *std::move(future).get());
  }

  return std::move(future).thenValue(
      [this, path = path.copy()](std::shared_ptr<const Tree>&& tree) {
        return diffRemovedTree(path, *tree);
      });
}

/**
 * Process a Tree that is present only on one side of the diff.
 */
Future<Unit> TreeDiffer::diffAddedTree(
    RelativePathPiece path,
    const Tree& tree) {
  ChildFutures childFutures;
  for (const auto& childEntry : tree.getTreeEntries()) {
    processAddedSide(childFutures, path, childEntry);
  }
  return waitOnResults(std::move(childFutures));
}

Future<Unit> TreeDiffer::diffRemovedTree(
    RelativePathPiece path,
    const Tree& tree) {
  ChildFutures childFutures;
  for (const auto& childEntry : tree.getTreeEntries()) {
    processRemovedSide(childFutures, path, childEntry);
  }
  return waitOnResults(std::move(childFutures));
}

/**
 * Process a TreeEntry that is present only on one side of the diff.
 * We don't know yet if this TreeEntry refers to a Tree or a Blob.
 *
 * If we could not compute a result immediately we will add an entry to
 * childFutures.
 */
void TreeDiffer::processRemovedSide(
    ChildFutures& childFutures,
    RelativePathPiece parentPath,
    const TreeEntry& entry) {
  if (!entry.isTree()) {
    callback_->removedFile(parentPath + entry.getName());
    return;
  }
  auto childPath = parentPath + entry.getName();
  auto childFuture = diffRemovedTree(childPath, entry.getHash());
  childFutures.add(std::move(childPath), std::move(childFuture));
}

/**
 * Process a TreeEntry that is present only on one side of the diff.
 * We don't know yet if this TreeEntry refers to a Tree or a Blob.
 *
 * If we could not compute a result immediately we will add an entry to
 * childFutures.
 */
void TreeDiffer::processAddedSide(
    ChildFutures& childFutures,
    RelativePathPiece parentPath,
    const TreeEntry& entry) {
  if (!entry.isTree()) {
    callback_->addedFile(parentPath + entry.getName());
    return;
  }
  auto childPath = parentPath + entry.getName();
  auto childFuture = diffAddedTree(childPath, entry.getHash());
  childFutures.add(std::move(childPath), std::move(childFuture));
}

/**
 * Process TreeEntry objects that exist on both sides of the diff.
 */
void TreeDiffer::processBothPresent(
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
          diffTrees(childPath, entry1.getHash(), entry2.getHash());
      childFutures.add(std::move(childPath), std::move(childFuture));
    } else {
      // tree-to-file
      // Record an ADDED entry for this path
      callback_->addedFile(parentPath + entry1.getName());
      // Report everything in tree1 as REMOVED
      processRemovedSide(childFutures, parentPath, entry1);
    }
  } else {
    if (isTree2) {
      // file-to-tree
      // Add a REMOVED entry for this path
      callback_->removedFile(parentPath + entry1.getName());

      // Report everything in tree2 as ADDED
      processAddedSide(childFutures, parentPath, entry2);
    } else {
      // file-to-file diff
      // We currently do not load the blob contents, and assume that blobs with
      // different hashes have different contents.
      if (entry1.getType() != entry2.getType() ||
          entry1.getHash() != entry2.getHash()) {
        callback_->modifiedFile(parentPath + entry1.getName());
      }
    }
  }
}

Future<Unit> TreeDiffer::waitOnResults(ChildFutures&& childFutures) {
  DCHECK_EQ(childFutures.paths.size(), childFutures.futures.size());
  if (childFutures.futures.empty()) {
    return makeFuture();
  }

  return folly::collectAllSemiFuture(std::move(childFutures.futures))
      .toUnsafeFuture()
      .thenValue([this, paths = std::move(childFutures.paths)](
                     vector<Try<Unit>>&& results) {
        DCHECK_EQ(paths.size(), results.size());
        for (size_t idx = 0; idx < results.size(); ++idx) {
          const auto& result = results[idx];
          if (!result.hasException()) {
            continue;
          }
          XLOG(ERR) << "error computing SCM diff for " << paths.at(idx);
          callback_->diffError(paths.at(idx), result.exception());
        }
      });
}

struct DiffState {
  explicit DiffState(ObjectStore* store)
      : callback(), differ(store, &callback) {}

  ScmStatusDiffCallback callback;
  TreeDiffer differ;
};
} // namespace

Future<std::unique_ptr<ScmStatus>>
diffCommitsForStatus(ObjectStore* store, Hash hash1, Hash hash2) {
  return folly::makeFutureWith([&] {
    auto state = std::make_unique<DiffState>(store);
    auto statePtr = state.get();
    return statePtr->differ.diffCommits(hash1, hash2)
        .thenValue([state = std::move(state)](auto&&) {
          return std::make_unique<ScmStatus>(state->callback.extractStatus());
        });
  });
}

Future<Unit>
diffTrees(ObjectStore* store, DiffCallback* callback, Hash tree1, Hash tree2) {
  return folly::makeFutureWith([&] {
    auto differ = make_unique<TreeDiffer>(store, callback);
    auto* differRawPtr = differ.get();
    return differRawPtr->diffTrees(RelativePathPiece{}, tree1, tree2)
        .thenValue(
            [differ = std::move(differ)](auto&&) { return makeFuture(); });
  });
}

Future<Unit> diffTrees(
    ObjectStore* store,
    DiffCallback* callback,
    const Tree& tree1,
    const Tree& tree2) {
  return folly::makeFutureWith([&] {
    auto differ = make_unique<TreeDiffer>(store, callback);
    auto* differRawPtr = differ.get();
    return differRawPtr->diffTrees(RelativePathPiece{}, tree1, tree2)
        .thenValue(
            [differ = std::move(differ)](auto&&) { return makeFuture(); });
  });
}

} // namespace eden
} // namespace facebook
