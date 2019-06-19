/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once
#include <folly/futures/Future.h>
#include <ostream>
#include "eden/fs/inodes/InodePtrFwd.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/git/GlobMatcher.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/utils/DirType.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook {
namespace eden {

/** Represents the compiled state of a tree-walking glob operation.
 * We split the glob into path components and build a tree of name
 * matching operations.
 * For non-recursive globs this allows an efficient walk and compare
 * as we work through the tree.  Path components that have no glob
 * special characters can be looked up directly from the directory
 * contents as a hash lookup, rather than by repeatedly matching the
 * pattern against each entry.
 */
class GlobNode {
 public:
  // Single parameter constructor is intended to create the root of a set of
  // globs that will be parsed into the overall glob tree.
  explicit GlobNode(bool includeDotfiles) : includeDotfiles_(includeDotfiles) {}

  using PrefetchList = std::shared_ptr<folly::Synchronized<std::vector<Hash>>>;

  GlobNode(folly::StringPiece pattern, bool includeDotfiles, bool hasSpecials);

  struct GlobResult {
    RelativePath name;
    dtype_t dtype;

    // Comparison operator for testing purposes
    bool operator==(const GlobResult& other) const noexcept {
      return name == other.name && dtype == other.dtype;
    }
    bool operator!=(const GlobResult& other) const noexcept {
      return !(*this == other);
    }
    GlobResult(RelativePathPiece name, dtype_t dtype)
        : name(name.copy()), dtype(dtype) {}

    GlobResult(RelativePath&& name, dtype_t dtype) noexcept
        : name(std::move(name)), dtype(dtype) {}
  };

  // Compile and add a new glob pattern to the tree.
  // Compilation splits the pattern into nodes, with one node for each
  // directory separator separated path component.
  void parse(folly::StringPiece pattern);

  // This is a recursive function to evaluate the compiled glob against
  // the provided input path and inode.
  // It returns the set of matching file names.
  // Note: the caller is responsible for ensuring that this
  // GlobNode exists until the returned Future is resolved.
  // If prefetchFiles is true, each matching file will have its content
  // prefetched via the ObjectStore layer.  This will not change the
  // materialization or overlay state for children that already have
  // inodes assigned.
  folly::Future<std::vector<GlobResult>> evaluate(
      const ObjectStore* store,
      RelativePathPiece rootPath,
      TreeInodePtr root,
      PrefetchList fileBlobsToPrefetch);

  // This is the Tree version of the method above
  folly::Future<std::vector<GlobResult>> evaluate(
      const ObjectStore* store,
      RelativePathPiece rootPath,
      const std::shared_ptr<const Tree>& tree,
      PrefetchList fileBlobsToPrefetch);

  /**
   * Print a human-readable description of this GlobNode to stderr.
   *
   * For debugging purposes only.
   */
  void debugDump() const;

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
  GlobNode* lookupToken(
      std::vector<std::unique_ptr<GlobNode>>* container,
      folly::StringPiece token);
  // Evaluates any recursive glob entries associated with this node.
  // This is a recursive function which evaluates the current GlobNode against
  // the recursive set of children.
  // By contrast, evaluate() walks down through the GlobNodes AND the
  // inode children.
  // The difference is because a pattern like "**/foo" must be recursively
  // matched against all the children of the inode.
  template <typename ROOT>
  folly::Future<std::vector<GlobResult>> evaluateRecursiveComponentImpl(
      const ObjectStore* store,
      RelativePathPiece rootPath,
      ROOT&& root,
      PrefetchList fileBlobsToPrefetch);

  template <typename ROOT>
  folly::Future<std::vector<GlobResult>> evaluateImpl(
      const ObjectStore* store,
      RelativePathPiece rootPath,
      ROOT&& root,
      PrefetchList fileBlobsToPrefetch);

  void debugDump(int currentDepth) const;

  // The pattern fragment for this node
  std::string pattern_;
  // The compiled pattern
  GlobMatcher matcher_;
  // List of non-** child rules
  std::vector<std::unique_ptr<GlobNode>> children_;
  // List of ** child rules
  std::vector<std::unique_ptr<GlobNode>> recursiveChildren_;

  // For a child GlobNode that is added to this GlobNode (presumably via
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

// Streaming operators for logging and printing
inline std::ostream& operator<<(
    std::ostream& stream,
    const GlobNode::GlobResult& a) {
  stream << "GlobResult{\"" << a.name.stringPiece()
         << "\", dtype=" << static_cast<int>(a.dtype) << "}";
  return stream;
}

} // namespace eden
} // namespace facebook
