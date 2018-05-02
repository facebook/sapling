/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once
#include <folly/futures/Future.h>
#include "eden/fs/inodes/InodePtrFwd.h"
#include "eden/fs/model/git/GlobMatcher.h"
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
  // Default constructor is intended to create the root of a set of globs
  // that will be parsed into the overall glob tree.
  GlobNode() = default;

  GlobNode(folly::StringPiece pattern, bool hasSpecials);

  // Compile and add a new glob pattern to the tree.
  // Compilation splits the pattern into nodes, with one node for each
  // directory separator separated path component.
  void parse(folly::StringPiece pattern);
  // This is a recursive function to evaluate the compiled glob against
  // the provided input path and inode.
  // It returns the set of matching file names.
  // Note: the caller is responsible for ensuring that this
  // GlobNode exists until the returned Future is resolved.
  folly::Future<std::unordered_set<RelativePath>> evaluate(
      RelativePathPiece rootPath,
      TreeInodePtr root);

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
  folly::Future<std::unordered_set<RelativePath>> evaluateRecursiveComponent(
      RelativePathPiece rootPath,
      TreeInodePtr root);
  // The pattern fragment for this node
  std::string pattern_;
  // The compiled pattern
  GlobMatcher matcher_;
  // List of non-** child rules
  std::vector<std::unique_ptr<GlobNode>> children_;
  // List of ** child rules
  std::vector<std::unique_ptr<GlobNode>> recursiveChildren_;

  // If true, generate results for matches.  Only applies
  // to non-recursive glob patterns.
  bool isLeaf_{false};
  // If false we can try a name lookup of pattern rather
  // than walking the children and applying the matcher
  bool hasSpecials_{false};
  // If true, this node is **
  bool alwaysMatch_{false};
};
} // namespace eden
} // namespace facebook
