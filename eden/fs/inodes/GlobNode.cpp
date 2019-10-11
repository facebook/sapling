/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "GlobNode.h"
#include <iomanip>
#include <iostream>
#include "eden/fs/inodes/TreeInode.h"

using folly::Future;
using folly::makeFuture;
using folly::StringPiece;
using std::make_unique;
using std::string;
using std::unique_ptr;
using std::vector;

namespace facebook {
namespace eden {

namespace {

// Policy objects to help avoid duplicating the core globbing logic.
// We can walk over two different kinds of trees; either TreeInodes
// or raw Trees from the storage layer.  While they have similar
// properties, accessing them is a little different.  These policy
// objects are thin shims that make access more uniform.

/** TreeInodePtrRoot wraps a TreeInodePtr for globbing.
 * TreeInodes require that a lock be held while its entries
 * are iterated.
 * We only need to prefetch children of TreeInodes that are
 * not materialized.
 */
struct TreeInodePtrRoot {
  TreeInodePtr root;

  explicit TreeInodePtrRoot(TreeInodePtr root) : root(root) {}

  /** Return an object that holds a lock over the children */
  auto lockContents() {
    return root->getContents().rlock();
  }

  /** Given the return value from lockContents and a name,
   * return a pointer to the child with that name, or nullptr
   * if there is no match */
  template <typename CONTENTS>
  const DirEntry* FOLLY_NULLABLE
  lookupEntry(CONTENTS& contents, PathComponentPiece name) {
    auto it = contents->entries.find(name);
    if (it != contents->entries.end()) {
      return &it->second;
    }
    return nullptr;
  }

  /** Return an object that can be used in a generic for()
   * constructor to iterate over the contents.  You must supply
   * the CONTENTS object you obtained via lockContents().
   * The returned iterator yields ENTRY elements that can be
   * used with the entryXXX methods below. */
  template <typename CONTENTS>
  auto& iterate(CONTENTS& contents) {
    return contents->entries;
  }

  /** Arrange to load a child TreeInode */
  Future<TreeInodePtr> getOrLoadChildTree(PathComponentPiece name) {
    return root->getOrLoadChildTree(name);
  }
  /** Returns true if we should call getOrLoadChildTree() for the given
   * ENTRY.  We only do this if the child is already materialized */
  template <typename ENTRY>
  bool entryShouldLoadChildTree(const ENTRY& entry) {
    return entry.second.isMaterialized();
  }
  bool entryShouldLoadChildTree(const DirEntry* entry) {
    return entry->isMaterialized();
  }

  /** Returns the name for a given ENTRY */
  template <typename ENTRY>
  PathComponentPiece entryName(const ENTRY& entry) {
    return entry.first;
  }

  /** Returns true if the given ENTRY is a tree */
  template <typename ENTRY>
  bool entryIsTree(const ENTRY& entry) {
    return entry.second.isDirectory();
  }

  /** Returns true if the given ENTRY is a tree (pointer version) */
  bool entryIsTree(const DirEntry* entry) {
    return entry->isDirectory();
  }

  /** Returns true if we should prefetch the blob content for the entry.
   * We only do this if the child is not already materialized */
  template <typename ENTRY>
  bool entryShouldPrefetch(const ENTRY& entry) {
    return !entry.second.isMaterialized() && !entryIsTree(entry);
  }
  bool entryShouldPrefetch(const DirEntry* entry) {
    return !entry->isMaterialized() && !entryIsTree(entry);
  }

  /** Returns the hash for the given ENTRY */
  template <typename ENTRY>
  const Hash entryHash(const ENTRY& entry) {
    return entry.second.getHash();
  }
  const Hash entryHash(const DirEntry* entry) {
    return entry->getHash();
  }

  template <typename ENTRY>
  GlobNode::GlobResult entryToResult(
      RelativePath&& entryPath,
      const ENTRY& entry) {
    return this->entryToResult(std::move(entryPath), &entry.second);
  }
  GlobNode::GlobResult entryToResult(
      RelativePath&& entryPath,
      const DirEntry* entry) {
    return GlobNode::GlobResult{std::move(entryPath), entry->getDtype()};
  }
};

/** TreeRoot wraps a Tree for globbing.
 * The entries do not need to be locked, but to satisfy the interface
 * we return the entries when lockContents() is called.
 */
struct TreeRoot {
  std::shared_ptr<const Tree> tree;

  explicit TreeRoot(const std::shared_ptr<const Tree>& tree) : tree(tree) {}

  /** We don't need to lock the contents, so we just return a reference
   * to the entries */
  auto& lockContents() {
    return tree->getTreeEntries();
  }

  /** Return an object that can be used in a generic for()
   * constructor to iterate over the contents.  You must supply
   * the CONTENTS object you obtained via lockContents().
   * The returned iterator yields ENTRY elements that can be
   * used with the entryXXX methods below. */
  template <typename CONTENTS>
  auto& iterate(CONTENTS& contents) {
    return contents;
  }

  /** We can never load a TreeInodePtr from a raw Tree, so this always
   * fails.  We never call this method because entryShouldLoadChildTree()
   * always returns false. */
  folly::Future<TreeInodePtr> getOrLoadChildTree(PathComponentPiece) {
    throw std::runtime_error("impossible to get here");
  }
  template <typename ENTRY>
  bool entryShouldLoadChildTree(const ENTRY&) {
    return false;
  }

  template <typename CONTENTS>
  auto* FOLLY_NULLABLE lookupEntry(CONTENTS&, PathComponentPiece name) {
    return tree->getEntryPtr(name);
  }

  template <typename ENTRY>
  PathComponentPiece entryName(const ENTRY& entry) {
    return entry.getName();
  }
  template <typename ENTRY>
  bool entryIsTree(const ENTRY& entry) {
    return entry.isTree();
  }
  bool entryIsTree(const TreeEntry* entry) {
    return entry->isTree();
  }

  // We always need to prefetch file children of a raw Tree
  template <typename ENTRY>
  bool entryShouldPrefetch(const ENTRY& entry) {
    return !entryIsTree(entry);
  }

  template <typename ENTRY>
  const Hash entryHash(const ENTRY& entry) {
    return entry.getHash();
  }
  const Hash entryHash(const TreeEntry* entry) {
    return entry->getHash();
  }

  GlobNode::GlobResult entryToResult(
      RelativePath&& entryPath,
      const TreeEntry* entry) {
    return GlobNode::GlobResult{std::move(entryPath), entry->getDType()};
  }

  GlobNode::GlobResult entryToResult(
      RelativePath&& entryPath,
      const TreeEntry& entry) {
    return this->entryToResult(std::move(entryPath), &entry);
  }
};
} // namespace

GlobNode::GlobNode(StringPiece pattern, bool includeDotfiles, bool hasSpecials)
    : pattern_(pattern.str()),
      includeDotfiles_(includeDotfiles),
      hasSpecials_(hasSpecials) {
  if (includeDotfiles && (pattern == "**" || pattern == "*")) {
    alwaysMatch_ = true;
  } else {
    auto options =
        includeDotfiles ? GlobOptions::DEFAULT : GlobOptions::IGNORE_DOTFILES;
    auto compiled = GlobMatcher::create(pattern, options);
    if (compiled.hasError()) {
      throw std::system_error(
          EINVAL,
          std::generic_category(),
          folly::sformat(
              "failed to compile pattern `{}` to GlobMatcher: {}",
              pattern,
              compiled.error()));
    }
    matcher_ = std::move(compiled.value());
  }
}

void GlobNode::parse(StringPiece pattern) {
  GlobNode* parent = this;
  string normalizedPattern;

  while (!pattern.empty()) {
    StringPiece token;
    auto* container = &parent->children_;
    bool hasSpecials;

    if (pattern.startsWith("**")) {
      // Recursive match defeats most optimizations; we have to stop
      // tokenizing here.

      // HACK: We special-case "**" if includeDotfiles=false. In this case, we
      // need to create a GlobMatcher for this pattern, but GlobMatcher is
      // designed to reject "**". As a workaround, we use "**/*", which is
      // functionally equivalent in this case because there are no other
      // "tokens" in the pattern following the "**" at this point.
      if (pattern == "**" && !includeDotfiles_) {
        normalizedPattern = "**/*";
        token = normalizedPattern;
      } else {
        token = pattern;
      }

      pattern = StringPiece();
      container = &parent->recursiveChildren_;
      hasSpecials = true;
    } else {
      token = tokenize(pattern, &hasSpecials);
    }

    auto node = lookupToken(container, token);
    if (!node) {
      container->emplace_back(
          std::make_unique<GlobNode>(token, includeDotfiles_, hasSpecials));
      node = container->back().get();
    }

    // If there are no more tokens remaining then we have a leaf node
    // that will emit results.  Update the node to reflect this.
    // Note that this may convert a pre-existing node from an earlier
    // glob specification to a leaf node.
    if (pattern.empty()) {
      node->isLeaf_ = true;
    }

    // Continue parsing the remainder of the pattern using this
    // (possibly new) node as the parent.
    parent = node;
  }
}

template <typename ROOT>
Future<vector<GlobNode::GlobResult>> GlobNode::evaluateImpl(
    const ObjectStore* store,
    RelativePathPiece rootPath,
    ROOT&& root,
    GlobNode::PrefetchList fileBlobsToPrefetch) {
  vector<GlobResult> results;
  vector<std::pair<PathComponentPiece, GlobNode*>> recurse;
  vector<Future<vector<GlobResult>>> futures;
  futures.emplace_back(evaluateRecursiveComponentImpl(
      store, rootPath, root, fileBlobsToPrefetch));

  auto recurseIfNecessary = [&](PathComponentPiece name,
                                GlobNode* node,
                                const auto& entry) {
    if ((!node->children_.empty() || !node->recursiveChildren_.empty()) &&
        root.entryIsTree(entry)) {
      if (root.entryShouldLoadChildTree(entry)) {
        recurse.emplace_back(std::make_pair(name, node));
      } else {
        auto candidateName = rootPath + name;
        futures.emplace_back(
            store->getTree(root.entryHash(entry))
                .thenValue([candidateName,
                            store,
                            innerNode = node,
                            fileBlobsToPrefetch](
                               std::shared_ptr<const Tree> dir) {
                  return innerNode->evaluateImpl(
                      store, candidateName, TreeRoot(dir), fileBlobsToPrefetch);
                }));
      }
    }
  };

  {
    auto contents = root.lockContents();
    for (auto& node : children_) {
      if (!node->hasSpecials_) {
        // We can try a lookup for the exact name
        auto name = PathComponentPiece(node->pattern_);
        auto entry = root.lookupEntry(contents, name);
        if (entry) {
          // Matched!
          if (node->isLeaf_) {
            results.emplace_back(root.entryToResult(rootPath + name, entry));

            if (fileBlobsToPrefetch && root.entryShouldPrefetch(entry)) {
              fileBlobsToPrefetch->wlock()->emplace_back(root.entryHash(entry));
            }
          }

          // Not the leaf of a pattern; if this is a dir, we need to recurse
          recurseIfNecessary(name, node.get(), entry);
        }
      } else {
        // We need to match it out of the entries in this inode
        for (auto& entry : root.iterate(contents)) {
          auto name = root.entryName(entry);
          if (node->alwaysMatch_ || node->matcher_.match(name.stringPiece())) {
            if (node->isLeaf_) {
              results.emplace_back(root.entryToResult(rootPath + name, entry));
              if (fileBlobsToPrefetch && root.entryShouldPrefetch(entry)) {
                fileBlobsToPrefetch->wlock()->emplace_back(
                    root.entryHash(entry));
              }
            }
            // Not the leaf of a pattern; if this is a dir, we need to
            // recurse
            recurseIfNecessary(name, node.get(), entry);
          }
        }
      }
    }
  }

  // Recursively load child inodes and evaluate matches

  for (auto& item : recurse) {
    auto candidateName = rootPath + item.first;
    futures.emplace_back(
        root.getOrLoadChildTree(item.first)
            .thenValue(
                [store, candidateName, node = item.second, fileBlobsToPrefetch](
                    TreeInodePtr dir) {
                  return node->evaluateImpl(
                      store,
                      candidateName,
                      TreeInodePtrRoot(dir),
                      fileBlobsToPrefetch);
                }));
  }

  // Note: we use collectAll() rather than collect() here to make sure that
  // we have really finished all computation before we return a result.
  // Our caller may destroy us after we return, so we can't let errors propagate
  // back to the caller early while some processing may still be occurring.
  return folly::collectAll(futures).thenValue(
      [shadowResults = std::move(results)](
          vector<folly::Try<vector<GlobNode::GlobResult>>>&&
              matchVector) mutable {
        for (auto& matches : matchVector) {
          matches.throwIfFailed();
          shadowResults.insert(
              shadowResults.end(),
              std::make_move_iterator(matches->begin()),
              std::make_move_iterator(matches->end()));
        }
        return shadowResults;
      });
}

Future<vector<GlobNode::GlobResult>> GlobNode::evaluate(
    const ObjectStore* store,
    RelativePathPiece rootPath,
    TreeInodePtr root,
    GlobNode::PrefetchList fileBlobsToPrefetch) {
  return evaluateImpl(
      store, rootPath, TreeInodePtrRoot(root), fileBlobsToPrefetch);
}

folly::Future<vector<GlobNode::GlobResult>> GlobNode::evaluate(
    const ObjectStore* store,
    RelativePathPiece rootPath,
    const std::shared_ptr<const Tree>& tree,
    GlobNode::PrefetchList fileBlobsToPrefetch) {
  return evaluateImpl(store, rootPath, TreeRoot(tree), fileBlobsToPrefetch);
}

StringPiece GlobNode::tokenize(StringPiece& pattern, bool* hasSpecials) {
  *hasSpecials = false;

  for (auto it = pattern.begin(); it != pattern.end(); ++it) {
    switch (*it) {
      case '*':
      case '?':
      case '[':
      case '\\':
        *hasSpecials = true;
        break;
      case '/':
        // token is the input up-to-but-not-including the current position,
        // which is a '/' character
        StringPiece token(pattern.begin(), it);
        // update the pattern to be the text after the slash
        pattern = StringPiece(it + 1, pattern.end());
        return token;
    }
  }

  // No slash found, so the the rest of the pattern is the token
  StringPiece token = pattern;
  pattern = StringPiece();
  return token;
}

GlobNode* GlobNode::lookupToken(
    vector<unique_ptr<GlobNode>>* container,
    StringPiece token) {
  for (auto& child : *container) {
    if (child->pattern_ == token) {
      return child.get();
    }
  }
  return nullptr;
}

template <typename ROOT>
Future<vector<GlobNode::GlobResult>> GlobNode::evaluateRecursiveComponentImpl(
    const ObjectStore* store,
    RelativePathPiece rootPath,
    ROOT&& root,
    GlobNode::PrefetchList fileBlobsToPrefetch) {
  vector<GlobResult> results;
  if (recursiveChildren_.empty()) {
    return results;
  }

  vector<RelativePath> subDirNames;
  vector<Future<vector<GlobResult>>> futures;
  {
    auto contents = root.lockContents();
    for (auto& entry : root.iterate(contents)) {
      auto candidateName = rootPath + root.entryName(entry);

      for (auto& node : recursiveChildren_) {
        if (node->alwaysMatch_ ||
            node->matcher_.match(candidateName.stringPiece())) {
          results.emplace_back(root.entryToResult(candidateName.copy(), entry));
          if (fileBlobsToPrefetch && root.entryShouldPrefetch(entry)) {
            fileBlobsToPrefetch->wlock()->emplace_back(root.entryHash(entry));
          }
          // No sense running multiple matches for this same file.
          break;
        }
      }

      // Remember to recurse through child dirs after we've released
      // the lock on the contents.
      if (root.entryIsTree(entry)) {
        if (root.entryShouldLoadChildTree(entry)) {
          subDirNames.emplace_back(candidateName);
        } else {
          futures.emplace_back(
              store->getTree(root.entryHash(entry))
                  .thenValue([candidateName, store, this, fileBlobsToPrefetch](
                                 const std::shared_ptr<const Tree>& tree) {
                    return evaluateRecursiveComponentImpl(
                        store,
                        candidateName,
                        TreeRoot(tree),
                        fileBlobsToPrefetch);
                  }));
        }
      }
    }
  }

  // Recursively load child inodes and evaluate matches
  for (auto& candidateName : subDirNames) {
    futures.emplace_back(
        root.getOrLoadChildTree(candidateName.basename())
            .thenValue([candidateName, store, this, fileBlobsToPrefetch](
                           TreeInodePtr dir) {
              return evaluateRecursiveComponentImpl(
                  store,
                  candidateName,
                  TreeInodePtrRoot(dir),
                  fileBlobsToPrefetch);
            }));
  }

  // Note: we use collectAll() rather than collect() here to make sure that
  // we have really finished all computation before we return a result.
  // Our caller may destroy us after we return, so we can't let errors propagate
  // back to the caller early while some processing may still be occurring.
  return folly::collectAll(futures).thenValue(
      [shadowResults = std::move(results)](
          vector<folly::Try<vector<GlobResult>>>&& matchVector) mutable {
        for (auto& matches : matchVector) {
          // Rethrow the exception if any of the results failed
          matches.throwIfFailed();
          shadowResults.insert(
              shadowResults.end(),
              std::make_move_iterator(matches->begin()),
              std::make_move_iterator(matches->end()));
        }
        return shadowResults;
      });
}

void GlobNode::debugDump() const {
  debugDump(/*currentDepth=*/0);
}

namespace {
struct Indentation {
  int width;

  friend std::ostream& operator<<(
      std::ostream& s,
      const Indentation& indentation) {
    return s << std::setw(indentation.width) << "";
  }
};
} // namespace

void GlobNode::debugDump(int currentDepth) const {
  auto& out = std::cerr;
  auto indentation = Indentation{currentDepth * 2};
  auto boolString = [](bool b) -> const char* { return b ? "true" : "false"; };

  out << indentation << "- GlobNode " << this << "\n"
      << indentation << "  alwaysMatch=" << boolString(alwaysMatch_) << "\n"
      << indentation << "  hasSpecials=" << boolString(hasSpecials_) << "\n"
      << indentation << "  includeDotfiles=" << boolString(includeDotfiles_)
      << "\n"
      << indentation << "  isLeaf=" << boolString(isLeaf_) << "\n";

  if (pattern_.empty()) {
    out << indentation << "  pattern is empty\n";
  } else {
    out << indentation << "  pattern: " << pattern_ << "\n";
  }

  if (!children_.empty()) {
    out << indentation << "  children (" << children_.size() << "):\n";
    for (const auto& child : children_) {
      child->debugDump(/*currentDepth=*/currentDepth + 1);
    }
  }

  if (!recursiveChildren_.empty()) {
    out << indentation << "  recursiveChildren (" << recursiveChildren_.size()
        << "):\n";
    for (const auto& child : recursiveChildren_) {
      child->debugDump(/*currentDepth=*/currentDepth + 1);
    }
  }
}

} // namespace eden
} // namespace facebook
