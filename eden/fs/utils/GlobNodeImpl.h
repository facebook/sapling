/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/futures/Future.h>

#include "eden/common/utils/CaseSensitivity.h"
#include "eden/common/utils/DirType.h"
#include "eden/common/utils/EnumValue.h"
#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/utils/GlobMatcher.h"

#include "eden/fs/telemetry/TaskTrace.h"
#include "eden/fs/utils/GlobResult.h"

namespace facebook::eden {

/**
 * Represents the compiled state of a tree-walking glob operation.
 *
 * We split the glob into path components and build a tree of name
 * matching operations.
 *
 * For non-recursive globs this allows an efficient walk and compare
 * as we work through the tree.  Path components that have no glob
 * special characters can be looked up directly from the directory
 * contents as an id lookup, rather than by repeatedly matching the
 * pattern against each entry.
 */
class GlobNodeImpl {
 public:
  // Two-parameter constructor is intended to create the root of a set of
  // globs that will be parsed into the overall glob tree.
  explicit GlobNodeImpl(bool includeDotfiles, CaseSensitivity caseSensitive)
      : caseSensitive_(caseSensitive), includeDotfiles_(includeDotfiles) {}

  virtual ~GlobNodeImpl() = default;

  using PrefetchList = folly::Synchronized<std::vector<ObjectId>>;

  GlobNodeImpl(
      folly::StringPiece pattern,
      bool includeDotfiles,
      bool hasSpecials,
      CaseSensitivity caseSensitive);

  // Compile and add a new glob pattern to the tree.
  // Compilation splits the pattern into nodes, with one node for each
  // directory separator separated path component.
  virtual void parse(folly::StringPiece pattern) final;

  /**
   * Print a human-readable description of this GlobNodeImpl to stderr.
   *
   * For debugging purposes only.
   */
  void debugDump() const;

  using TreeRootPtr = std::shared_ptr<const Tree>;

 protected:
  /** TreeRoot wraps a Tree for globbing.
   * The entries do not need to be locked, but to satisfy the interface
   * we return the entries when lockContents() is called.
   */
  struct TreeRoot {
    std::shared_ptr<const Tree> tree;

    explicit TreeRoot(std::shared_ptr<const Tree> entries)
        : tree(std::move(entries)) {}

    /** We don't need to lock the contents, so we just return a reference
     * to the entries */
    const Tree& lockContents() {
      return *tree;
    }

    /** Return an object that can be used in a generic for()
     * constructor to iterate over the contents.  You must supply
     * the object you obtained via lockContents().
     * The returned iterator yields ENTRY elements that can be
     * used with the entryXXX methods below. */
    const Tree& iterate(const Tree& entries) {
      return entries;
    }

    /** We can never load a TreeInodePtr from a raw Tree, so this always
     * fails.  We never call this method because entryShouldLoadChildTree()
     * always returns false. */
    ImmediateFuture<TreeRootPtr> getOrLoadChildTree(
        PathComponentPiece,
        const ObjectFetchContextPtr&) {
      throw std::runtime_error("impossible to get here");
    }

    bool entryShouldLoadChildTree(const TreeEntry*) {
      return false;
    }

    typename Tree::container::const_pointer FOLLY_NULLABLE
    lookupEntry(const Tree& entries, PathComponentPiece name) {
      auto it = entries.find(name);
      if (it != entries.cend()) {
        return &*it;
      }
      return nullptr;
    }

    bool entryIsTree(const TreeEntry* entry) {
      return entry->isTree();
    }

    // We always need to prefetch file children of a raw Tree
    bool entryShouldPrefetch(const TreeEntry* entry) {
      return !entryIsTree(entry);
    }
  };
  // Evaluates any recursive glob entries associated with this node.
  // This is a recursive function which evaluates the current GlobNodeImpl
  // against the recursive set of children. By contrast, evaluate() walks down
  // through the GlobNodeImpls AND the inode children. The difference is because
  // a pattern like "**/foo" must be recursively matched against all the
  // children of the inode.
  template <typename ROOT, typename ROOTPtr>
  ImmediateFuture<folly::Unit> evaluateRecursiveComponentImpl(
      const ObjectStore* store,
      const ObjectFetchContextPtr& context,
      RelativePathPiece rootPath,
      RelativePathPiece startOfRecursive,
      ROOT&& root,
      PrefetchList* fileBlobsToPrefetch,
      ResultList& globResult,
      const RootId& originRootId) const {
    TaskTraceBlock block{"GlobNodeImpl::evaluateRecursiveComponentImpl"};
    std::vector<RelativePath> subDirNames;
    std::vector<ImmediateFuture<folly::Unit>> futures;
    {
      const auto& contents = root.lockContents();
      for (auto& entry : root.iterate(contents)) {
        auto candidateName = startOfRecursive + entry.first;

        for (auto& node : recursiveChildren_) {
          if (node->alwaysMatch_ ||
              node->matcher_.match(candidateName.view())) {
            globResult.wlock()->emplace_back(
                rootPath + candidateName,
                entry.second.getDtype(),
                originRootId);
            if (fileBlobsToPrefetch &&
                root.entryShouldPrefetch(&entry.second)) {
              fileBlobsToPrefetch->wlock()->emplace_back(
                  entry.second.getObjectId());
            }
            // No sense running multiple matches for this same file.
            break;
          }
        }

        // Remember to recurse through child dirs after we've released
        // the lock on the contents.
        if (root.entryIsTree(&entry.second)) {
          if (root.entryShouldLoadChildTree(&entry.second)) {
            subDirNames.emplace_back(std::move(candidateName));
          } else {
            futures.emplace_back(
                store->getTree(entry.second.getObjectId(), context)
                    .thenValue(
                        [candidateName = std::move(candidateName),
                         rootPath = rootPath.copy(),
                         store,
                         context = context.copy(),
                         this,
                         fileBlobsToPrefetch,
                         &globResult,
                         &originRootId](std::shared_ptr<const Tree> tree) {
                          return evaluateRecursiveComponentImpl<
                              TreeRoot,
                              TreeRootPtr>(
                              store,
                              context,
                              rootPath,
                              candidateName,
                              TreeRoot(std::move(tree)),
                              fileBlobsToPrefetch,
                              globResult,
                              originRootId);
                        }));
          }
        }
      }
    }

    // Recursively load child inodes and evaluate matches
    for (auto& candidateName : subDirNames) {
      auto childTreeFuture =
          root.getOrLoadChildTree(candidateName.basename(), context);
      futures.emplace_back(
          std::move(childTreeFuture)
              .thenValue([candidateName = std::move(candidateName),
                          rootPath = rootPath.copy(),
                          store,
                          context = context.copy(),
                          this,
                          fileBlobsToPrefetch,
                          &globResult,
                          &originRootId](ROOTPtr dir) {
                return evaluateRecursiveComponentImpl<ROOT, ROOTPtr>(
                    store,
                    context,
                    rootPath,
                    candidateName,
                    ROOT(std::move(dir)),
                    fileBlobsToPrefetch,
                    globResult,
                    originRootId);
              }));
    }

    // Note: we use collectAll() rather than collect() here to make sure that
    // we have really finished all computation before we return a result.
    // Our caller may destroy us after we return, so we can't let errors
    // propagate back to the caller early while some processing may still be
    // occurring.
    return collectAll(std::move(futures))
        .thenValue([](std::vector<folly::Try<folly::Unit>>&& results) {
          for (auto& result : results) {
            // Rethrow the exception if any of the results failed
            result.throwUnlessValue();
          }
          return folly::unit;
        });
  }

  template <typename ROOT, typename ROOTPtr>
  ImmediateFuture<folly::Unit> evaluateImpl(
      const ObjectStore* store,
      const ObjectFetchContextPtr& context,
      RelativePathPiece rootPath,
      ROOT&& root,
      PrefetchList* fileBlobsToPrefetch,
      ResultList& globResult,
      const RootId& originRootId) const {
    TaskTraceBlock block{"GlobNodeImpl::evaluateImpl"};
    std::vector<std::pair<PathComponentPiece, GlobNodeImpl*>> recurse;
    std::vector<ImmediateFuture<folly::Unit>> futures;

    if (!recursiveChildren_.empty()) {
      futures.emplace_back(evaluateRecursiveComponentImpl<ROOT, ROOTPtr>(
          store,
          context,
          rootPath,
          RelativePathPiece{""},
          std::forward<ROOT>(root),
          fileBlobsToPrefetch,
          globResult,
          originRootId));
    }

    auto recurseIfNecessary = [&](PathComponentPiece name,
                                  GlobNodeImpl* node,
                                  const auto& entry) {
      TaskTraceBlock block2{"GlobNodeImpl::evaluateImpl::recurseIfNecessary"};
      if ((!node->children_.empty() || !node->recursiveChildren_.empty()) &&
          root.entryIsTree(entry)) {
        if (root.entryShouldLoadChildTree(entry)) {
          recurse.emplace_back(name, node);
        } else {
          futures.emplace_back(
              store->getTree(entry->getObjectId(), context)
                  .thenValue(
                      [candidateName = rootPath + name,
                       store,
                       context = context.copy(),
                       innerNode = node,
                       fileBlobsToPrefetch,
                       &globResult,
                       &originRootId](std::shared_ptr<const Tree> dir) mutable {
                        return innerNode->evaluateImpl<TreeRoot, TreeRootPtr>(
                            store,
                            context,
                            candidateName,
                            TreeRoot(std::move(dir)),
                            fileBlobsToPrefetch,
                            globResult,
                            originRootId);
                      }));
        }
      }
    };

    {
      const auto& contents = root.lockContents();
      for (auto& node : children_) {
        if (!node->hasSpecials_) {
          // We can try a lookup for the exact name
          PathComponentPiece name{node->pattern_};
          auto entry = root.lookupEntry(contents, name);
          if (entry) {
            // Matched!

            // Update the name to reflect the entry's actual case
            name = entry->first;

            if (node->isLeaf_) {
              globResult.wlock()->emplace_back(
                  rootPath + name, entry->second.getDtype(), originRootId);

              if (fileBlobsToPrefetch &&
                  root.entryShouldPrefetch(&entry->second)) {
                fileBlobsToPrefetch->wlock()->emplace_back(
                    entry->second.getObjectId());
              }
            }

            // Not the leaf of a pattern; if this is a dir, we need to recurse
            recurseIfNecessary(name, node.get(), &entry->second);
          }
        } else {
          // We need to match it out of the entries in this inode
          for (auto& entry : root.iterate(contents)) {
            PathComponentPiece name = entry.first;
            if (node->alwaysMatch_ || node->matcher_.match(name.view())) {
              if (node->isLeaf_) {
                globResult.wlock()->emplace_back(
                    rootPath + name, entry.second.getDtype(), originRootId);
                if (fileBlobsToPrefetch &&
                    root.entryShouldPrefetch(&entry.second)) {
                  fileBlobsToPrefetch->wlock()->emplace_back(
                      entry.second.getObjectId());
                }
              }
              // Not the leaf of a pattern; if this is a dir, we need to
              // recurse
              recurseIfNecessary(name, node.get(), &entry.second);
            }
          }
        }
      }
    }

    // Recursively load child inodes and evaluate matches

    for (auto& item : recurse) {
      futures.emplace_back(
          root.getOrLoadChildTree(item.first, context)
              .thenValue([store,
                          context = context.copy(),
                          candidateName = rootPath + item.first,
                          node = item.second,
                          fileBlobsToPrefetch,
                          &globResult,
                          &originRootId](ROOTPtr dir) {
                return node->evaluateImpl<ROOT, ROOTPtr>(
                    store,
                    context,
                    candidateName,
                    ROOT(std::move(dir)),
                    fileBlobsToPrefetch,
                    globResult,
                    originRootId);
              }));
    }

    // Note: we use collectAll() rather than collect() here to make sure that
    // we have really finished all computation before we return a result.
    // Our caller may destroy us after we return, so we can't let errors
    // propagate back to the caller early while some processing may still be
    // occurring.
    return collectAll(std::move(futures))
        .thenValue([](std::vector<folly::Try<folly::Unit>>&& results) {
          TaskTraceBlock block2{
              "GlobNodeImpl::evaluateImpl::collectAll::thenValue"};
          for (auto& result : results) {
            result.throwUnlessValue();
          }
          return folly::unit;
        });
  }

 private:
  // Returns the next glob node token.
  // This is the text from the start of pattern up to the first
  // slash, or the end of the string is there was no slash.
  // pattern is advanced to the start of the next token.
  // hasSpecials is set to true if the returned token contains
  // any special glob characters, false otherwise.
  static folly::StringPiece tokenize(
      folly::StringPiece& pattern,
      bool* hasSpecials);
  // Look up the child corresponding to a token.
  // Returns nullptr if it does not exist.
  // This is a simple brute force walk of the vector; the cardinality
  // of the glob nodes are typically very low so this is fine.
  GlobNodeImpl* lookupToken(
      std::vector<std::unique_ptr<GlobNodeImpl>>* container,
      folly::StringPiece token);

  void debugDump(int currentDepth) const;

  // The pattern fragment for this node
  std::string pattern_;
  // The compiled pattern
  GlobMatcher matcher_;
  // List of non-** child rules
  std::vector<std::unique_ptr<GlobNodeImpl>> children_;
  // List of ** child rules
  std::vector<std::unique_ptr<GlobNodeImpl>> recursiveChildren_;

  // The case sensitivity of this glob node.
  CaseSensitivity caseSensitive_;

  // For a child GlobNodeImpl that is added to this GlobNodeImpl (presumably via
  // parse()), the GlobMatcher pattern associated with the child node should use
  // this value for its includeDotfiles parameter.
  bool includeDotfiles_;
  // If true, generate results for matches.  Only applies
  // to non-recursive glob patterns.
  bool isLeaf_{false};
  // If false we can try a name lookup of pattern rather
  // than walking the children and applying the matcher
  bool hasSpecials_{false};
  // true when both of the following hold:
  // - this node is "**" or "*"
  // - it was created with includeDotfiles=true.
  bool alwaysMatch_{false};
};

} // namespace facebook::eden
