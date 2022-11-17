/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include <ostream>
#include "eden/fs/inodes/InodePtrFwd.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/git/GlobMatcher.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/utils/CaseSensitivity.h"
#include "eden/fs/utils/DirType.h"
#include "eden/fs/utils/EnumValue.h"
#include "eden/fs/utils/PathFuncs.h"

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
 * contents as a hash lookup, rather than by repeatedly matching the
 * pattern against each entry.
 */
class GlobNode {
 public:
  // Two-parameter constructor is intended to create the root of a set of
  // globs that will be parsed into the overall glob tree.
  explicit GlobNode(bool includeDotfiles, CaseSensitivity caseSensitive)
      : caseSensitive_(caseSensitive), includeDotfiles_(includeDotfiles) {}

  using PrefetchList = folly::Synchronized<std::vector<ObjectId>>;

  GlobNode(
      folly::StringPiece pattern,
      bool includeDotfiles,
      bool hasSpecials,
      CaseSensitivity caseSensitive);

  struct GlobResult {
    RelativePath name;
    dtype_t dtype;
    // Currently this is the commit hash for the commit to which this file
    // belongs. But should eden move away from commit hashes this may become
    // the tree hash of the root tree to which this file belongs.
    // This should never become a dangling reference because the caller
    // of Globresult::evaluate ensures that the hashes have a lifetime that
    // exceeds that of the GlobResults returned.
    const RootId* originHash;

    // Comparison operator for testing purposes
    bool operator==(const GlobResult& other) const noexcept {
      return name == other.name && dtype == other.dtype &&
          originHash == other.originHash;
    }
    bool operator!=(const GlobResult& other) const noexcept {
      return !(*this == other);
    }

    bool operator<(const GlobResult& other) const noexcept {
      return name < other.name || (name == other.name && dtype < other.dtype) ||
          (name == other.name && dtype == other.dtype &&
           originHash < other.originHash);
    }

    // originHash should never become a dangling refernece because the caller
    // of Globresult::evaluate ensures that the hashes have a lifetime that
    // exceeds that of the GlobResults returned.
    GlobResult(RelativePathPiece name, dtype_t dtype, const RootId& originHash)
        : name(name.copy()), dtype(dtype), originHash(&originHash) {}

    GlobResult(
        RelativePath&& name,
        dtype_t dtype,
        const RootId& originHash) noexcept
        : name(std::move(name)), dtype(dtype), originHash(&originHash) {}
  };

  using ResultList = folly::Synchronized<std::vector<GlobResult>>;

  // Compile and add a new glob pattern to the tree.
  // Compilation splits the pattern into nodes, with one node for each
  // directory separator separated path component.
  void parse(folly::StringPiece pattern);

  /**
   * Evaluate the compiled glob against the provided TreeInode and path.
   *
   * The results are appended to the globResult list which the caller is
   * responsible for ensuring that its lifetime will exceed the lifetime of the
   * returned ImmediateFuture.
   *
   * When fileBlobsToPrefetch is non-null, the Hash of the globbed files will
   * be appended to it.
   */
  ImmediateFuture<folly::Unit> evaluate(
      const ObjectStore* store,
      const ObjectFetchContextPtr& context,
      RelativePathPiece rootPath,
      TreeInodePtr root,
      PrefetchList* fileBlobsToPrefetch,
      ResultList& globResult,
      const RootId& originRootId) const;

  /**
   * Evaluate the compiled glob against the provided Tree.
   *
   * See the documention for the overload above.
   */
  ImmediateFuture<folly::Unit> evaluate(
      const ObjectStore* store,
      const ObjectFetchContextPtr& context,
      RelativePathPiece rootPath,
      std::shared_ptr<const Tree> tree,
      PrefetchList* fileBlobsToPrefetch,
      ResultList& globResult,
      const RootId& originRootId) const;

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
  ImmediateFuture<folly::Unit> evaluateRecursiveComponentImpl(
      const ObjectStore* store,
      const ObjectFetchContextPtr& context,
      RelativePathPiece rootPath,
      RelativePathPiece startOfRecursive,
      ROOT&& root,
      PrefetchList* fileBlobsToPrefetch,
      ResultList& globResult,
      const RootId& originRootId) const;

  template <typename ROOT>
  ImmediateFuture<folly::Unit> evaluateImpl(
      const ObjectStore* store,
      const ObjectFetchContextPtr& context,
      RelativePathPiece rootPath,
      ROOT&& root,
      PrefetchList* fileBlobsToPrefetch,
      ResultList& globResult,
      const RootId& originRootId) const;

  void debugDump(int currentDepth) const;

  // The pattern fragment for this node
  std::string pattern_;
  // The compiled pattern
  GlobMatcher matcher_;
  // List of non-** child rules
  std::vector<std::unique_ptr<GlobNode>> children_;
  // List of ** child rules
  std::vector<std::unique_ptr<GlobNode>> recursiveChildren_;

  // The case sensitivity of this glob node.
  CaseSensitivity caseSensitive_;

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
  stream << "GlobResult{\"" << a.name << "\", dtype=" << enumValue(a.dtype)
         << "}";
  return stream;
}

} // namespace facebook::eden
