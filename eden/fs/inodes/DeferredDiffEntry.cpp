/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/inodes/DeferredDiffEntry.h"

#include <folly/Unit.h>
#include <folly/futures/Future.h>
#include <folly/logging/xlog.h>
#include "eden/fs/inodes/DiffContext.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/InodeDiffCallback.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/store/BlobMetadata.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/utils/Bug.h"

using folly::Future;
using folly::makeFuture;
using folly::Unit;
using std::make_unique;
using std::shared_ptr;
using std::unique_ptr;
using std::vector;

namespace facebook {
namespace eden {

namespace {

class UntrackedDiffEntry : public DeferredDiffEntry {
 public:
  UntrackedDiffEntry(
      const DiffContext* context,
      RelativePath path,
      InodePtr inode,
      const GitIgnoreStack* ignore,
      bool isIgnored)
      : DeferredDiffEntry{context, std::move(path)},
        ignore_{ignore},
        isIgnored_{isIgnored},
        inode_{std::move(inode)} {}

  /*
   * This is a template just to avoid ambiguity with the prior constructor,
   * since folly::Future<X> can unfortunately be implicitly constructed from X.
   */
  template <
      typename InodeFuture,
      typename X = typename std::enable_if<
          std::is_same<folly::Future<InodePtr>, InodeFuture>::value,
          void>>
  UntrackedDiffEntry(
      const DiffContext* context,
      RelativePath path,
      InodeFuture&& inodeFuture,
      const GitIgnoreStack* ignore,
      bool isIgnored)
      : DeferredDiffEntry{context, std::move(path)},
        ignore_{ignore},
        isIgnored_{isIgnored},
        inodeFuture_{std::forward<InodeFuture>(inodeFuture)} {}

  folly::Future<folly::Unit> run() override {
    // If we have an inodeFuture_ to wait on, wait for it to finish,
    // then store the resulting inode_ and invoke run() again.
    if (inodeFuture_.valid()) {
      CHECK(!inode_) << "cannot have both inode_ and inodeFuture_ set";
      return std::move(inodeFuture_).thenValue([this](InodePtr inode) {
        inode_ = std::move(inode);
        inodeFuture_ = folly::Future<InodePtr>::makeEmpty();
        return run();
      });
    }

    auto treeInode = inode_.asTreePtrOrNull();
    if (!treeInode.get()) {
      auto bug = EDEN_BUG()
          << "UntrackedDiffEntry should only used with tree inodes";
      return makeFuture<Unit>(bug.toException());
    }

    // Recursively diff the untracked directory.
    return treeInode->diff(context_, getPath(), nullptr, ignore_, isIgnored_);
  }

 private:
  const GitIgnoreStack* ignore_{nullptr};
  bool isIgnored_{false};
  InodePtr inode_;
  folly::Future<InodePtr> inodeFuture_ = folly::Future<InodePtr>::makeEmpty();
};

/*
 * Helper functions for diffing removed directories.
 *
 * This is used by both RemovedDiffEntry and ModifiedEntryInfo.
 * (ModifiedBlobDiffEntry needs it for handling cases where a directory was
 * replaced with a file.)
 */
namespace {
// Overload that takes an already loaded Tree
folly::Future<folly::Unit> diffRemovedTree(
    const DiffContext* context,
    RelativePath currentPath,
    const Tree* tree);
// Overload that takes a TreeEntry, and has to load the Tree in question first
folly::Future<folly::Unit> diffRemovedTree(
    const DiffContext* context,
    RelativePath currentPath,
    const TreeEntry& entry);

folly::Future<folly::Unit> diffRemovedTree(
    const DiffContext* context,
    RelativePath currentPath,
    const TreeEntry& entry) {
  DCHECK(entry.isTree());
  return context->store->getTree(entry.getHash())
      .thenValue([context, currentPath = RelativePath{std::move(currentPath)}](
                     shared_ptr<const Tree>&& tree) {
        return diffRemovedTree(context, std::move(currentPath), tree.get());
      });
}

folly::Future<folly::Unit> diffRemovedTree(
    const DiffContext* context,
    RelativePath currentPath,
    const Tree* tree) {
  vector<Future<Unit>> subFutures;
  for (const auto& entry : tree->getTreeEntries()) {
    if (entry.isTree()) {
      auto f = diffRemovedTree(context, currentPath + entry.getName(), entry);
      subFutures.emplace_back(std::move(f));
    } else {
      XLOG(DBG5) << "diff: file in removed directory: "
                 << currentPath + entry.getName();
      context->callback->removedFile(currentPath + entry.getName(), entry);
    }
  }

  return folly::collectAllSemiFuture(subFutures)
      .toUnsafeFuture()
      .thenValue([currentPath = RelativePath{std::move(currentPath)},
                  tree = std::move(tree),
                  context](vector<folly::Try<Unit>> results) {
        // Call diffError() for each error that occurred
        for (size_t n = 0; n < results.size(); ++n) {
          auto& result = results[n];
          if (result.hasException()) {
            const auto& entry = tree->getEntryAt(n);
            XLOG(WARN) << "exception processing diff for "
                       << currentPath + entry.getName() << ": "
                       << folly::exceptionStr(result.exception());
            context->callback->diffError(
                currentPath + entry.getName(), result.exception());
          }
        }
        // Return successfully after recording the errors.  (If we failed then
        // our caller would also record us as an error, which we don't want.)
        return makeFuture();
      });
}
} // unnamed namespace

class RemovedDiffEntry : public DeferredDiffEntry {
 public:
  RemovedDiffEntry(
      const DiffContext* context,
      RelativePath path,
      const TreeEntry& scmEntry)
      : DeferredDiffEntry{context, std::move(path)}, scmEntry_{scmEntry} {
    // We only need to defer processing for removed directories;
    // we never create RemovedDiffEntry objects for removed files.
    DCHECK(scmEntry_.isTree());
  }

  folly::Future<folly::Unit> run() override {
    return diffRemovedTree(context_, getPath(), scmEntry_);
  }

 private:
  TreeEntry scmEntry_;
};

class ModifiedDiffEntry : public DeferredDiffEntry {
 public:
  ModifiedDiffEntry(
      const DiffContext* context,
      RelativePath path,
      const TreeEntry& scmEntry,
      InodePtr inode,
      const GitIgnoreStack* ignore,
      bool isIgnored)
      : DeferredDiffEntry{context, std::move(path)},
        ignore_{ignore},
        isIgnored_{isIgnored},
        scmEntry_{scmEntry},
        inode_{std::move(inode)} {}

  ModifiedDiffEntry(
      const DiffContext* context,
      RelativePath path,
      const TreeEntry& scmEntry,
      folly::Future<InodePtr>&& inodeFuture,
      const GitIgnoreStack* ignore,
      bool isIgnored)
      : DeferredDiffEntry{context, std::move(path)},
        ignore_{ignore},
        isIgnored_{isIgnored},
        scmEntry_{scmEntry},
        inodeFuture_{std::move(inodeFuture)} {}

  folly::Future<folly::Unit> run() override {
    // If we have an inodeFuture_, wait on it to complete.
    // TODO: Load the inode in parallel with loading the source control data
    // below.
    if (inodeFuture_.valid()) {
      CHECK(!inode_) << "cannot have both inode_ and inodeFuture_ set";
      return std::move(inodeFuture_).thenValue([this](InodePtr inode) {
        inode_ = std::move(inode);
        inodeFuture_ = Future<InodePtr>::makeEmpty();
        return run();
      });
    }

    if (scmEntry_.isTree()) {
      return runForScmTree();
    } else {
      return runForScmBlob();
    }
  }

 private:
  folly::Future<folly::Unit> runForScmTree() {
    auto treeInode = inode_.asTreePtrOrNull();
    if (!treeInode) {
      // This is a Tree in the source control state, but a file or symlink
      // in the current filesystem state.
      // Report this file as untracked, and everything in the source control
      // tree as removed.
      if (isIgnored_) {
        if (context_->listIgnored) {
          XLOG(DBG6) << "directory --> ignored file: " << getPath();
          context_->callback->ignoredFile(getPath());
        }
      } else {
        XLOG(DBG6) << "directory --> untracked file: " << getPath();
        context_->callback->untrackedFile(getPath());
      }
      return diffRemovedTree(context_, getPath(), scmEntry_);
    }

    {
      auto contents = treeInode->getContents().wlock();
      if (!contents->isMaterialized() &&
          contents->treeHash.value() == scmEntry_.getHash()) {
        // It did not change since it was loaded,
        // and it matches the scmEntry we're diffing against.
        return makeFuture();
      }
    }

    // Possibly modified directory.  Load the Tree in question.
    return context_->store->getTree(scmEntry_.getHash())
        .thenValue([this, treeInode = std::move(treeInode)](
                       shared_ptr<const Tree>&& tree) {
          return treeInode->diff(
              context_, getPath(), std::move(tree), ignore_, isIgnored_);
        });
  }

  folly::Future<folly::Unit> runForScmBlob() {
    auto fileInode = inode_.asFilePtrOrNull();
    if (!fileInode) {
      // This is a file in the source control state, but a directory
      // in the current filesystem state.
      // Report this file as removed, and everything in the source control
      // tree as untracked/ignored.
      XLOG(DBG5) << "removed file: " << getPath();
      context_->callback->removedFile(getPath(), scmEntry_);
      auto treeInode = inode_.asTreePtr();
      if (isIgnored_ && !context_->listIgnored) {
        return makeFuture();
      }
      return treeInode->diff(context_, getPath(), nullptr, ignore_, isIgnored_);
    }

    return fileInode->isSameAs(scmEntry_.getHash(), scmEntry_.getType())
        .thenValue([this](bool isSame) {
          if (!isSame) {
            XLOG(DBG5) << "modified file: " << getPath();
            context_->callback->modifiedFile(getPath(), scmEntry_);
          }
        });
  }

  const GitIgnoreStack* ignore_{nullptr};
  bool isIgnored_{false};
  TreeEntry scmEntry_;
  folly::Future<InodePtr> inodeFuture_ = folly::Future<InodePtr>::makeEmpty();
  InodePtr inode_;
  shared_ptr<const Tree> scmTree_;
};

class ModifiedBlobDiffEntry : public DeferredDiffEntry {
 public:
  ModifiedBlobDiffEntry(
      const DiffContext* context,
      RelativePath path,
      const TreeEntry& scmEntry,
      Hash currentBlobHash)
      : DeferredDiffEntry{context, std::move(path)},
        scmEntry_{scmEntry},
        currentBlobHash_{currentBlobHash} {}

  folly::Future<folly::Unit> run() override {
    auto f1 = context_->store->getBlobSha1(scmEntry_.getHash());
    auto f2 = context_->store->getBlobSha1(currentBlobHash_);
    return folly::collect(f1, f2).thenValue(
        [this](const std::tuple<Hash, Hash>& info) {
          if (std::get<0>(info) != std::get<1>(info)) {
            XLOG(DBG5) << "modified file: " << getPath();
            context_->callback->modifiedFile(getPath(), scmEntry_);
          }
        });
  }

 private:
  TreeEntry scmEntry_;
  Hash currentBlobHash_;
};

} // unnamed namespace

unique_ptr<DeferredDiffEntry> DeferredDiffEntry::createUntrackedEntry(
    const DiffContext* context,
    RelativePath path,
    InodePtr inode,
    const GitIgnoreStack* ignore,
    bool isIgnored) {
  return make_unique<UntrackedDiffEntry>(
      context, std::move(path), std::move(inode), ignore, isIgnored);
}

unique_ptr<DeferredDiffEntry>
DeferredDiffEntry::createUntrackedEntryFromInodeFuture(
    const DiffContext* context,
    RelativePath path,
    Future<InodePtr>&& inodeFuture,
    const GitIgnoreStack* ignore,
    bool isIgnored) {
  return make_unique<UntrackedDiffEntry>(
      context, std::move(path), std::move(inodeFuture), ignore, isIgnored);
}

unique_ptr<DeferredDiffEntry> DeferredDiffEntry::createRemovedEntry(
    const DiffContext* context,
    RelativePath path,
    const TreeEntry& scmEntry) {
  return make_unique<RemovedDiffEntry>(context, std::move(path), scmEntry);
}

unique_ptr<DeferredDiffEntry> DeferredDiffEntry::createModifiedEntry(
    const DiffContext* context,
    RelativePath path,
    const TreeEntry& scmEntry,
    InodePtr inode,
    const GitIgnoreStack* ignore,
    bool isIgnored) {
  return make_unique<ModifiedDiffEntry>(
      context, std::move(path), scmEntry, std::move(inode), ignore, isIgnored);
}

unique_ptr<DeferredDiffEntry>
DeferredDiffEntry::createModifiedEntryFromInodeFuture(
    const DiffContext* context,
    RelativePath path,
    const TreeEntry& scmEntry,
    folly::Future<InodePtr>&& inodeFuture,
    const GitIgnoreStack* ignore,
    bool isIgnored) {
  return make_unique<ModifiedDiffEntry>(
      context,
      std::move(path),
      scmEntry,
      std::move(inodeFuture),
      ignore,
      isIgnored);
}

unique_ptr<DeferredDiffEntry> DeferredDiffEntry::createModifiedEntry(
    const DiffContext* context,
    RelativePath path,
    const TreeEntry& scmEntry,
    Hash currentBlobHash) {
  return make_unique<ModifiedBlobDiffEntry>(
      context, std::move(path), scmEntry, currentBlobHash);
}
} // namespace eden
} // namespace facebook
