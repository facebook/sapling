/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "GlobNode.h"
#include "EdenError.h"
#include "eden/fs/inodes/TreeInode.h"

using folly::Future;
using folly::makeFuture;
using folly::StringPiece;
using std::make_unique;
using std::string;
using std::unique_ptr;
using std::unordered_set;
using std::vector;

namespace facebook {
namespace eden {

GlobNode::GlobNode(StringPiece pattern, bool hasSpecials)
    : pattern_(pattern.str()), hasSpecials_(hasSpecials) {
  if (pattern == "**" || pattern == "*") {
    alwaysMatch_ = true;
  } else {
    auto compiled = GlobMatcher::create(pattern, GlobOptions::DEFAULT);
    if (compiled.hasError()) {
      throw newEdenError(
          EINVAL,
          "failed to compile pattern `{}` to GlobMatcher: {}",
          pattern,
          compiled.error());
    }
    matcher_ = std::move(compiled.value());
  }
}

void GlobNode::parse(StringPiece pattern) {
  GlobNode* parent = this;

  while (!pattern.empty()) {
    StringPiece token;
    auto* container = &parent->children_;
    bool hasSpecials;

    if (pattern.startsWith("**")) {
      // Recursive match defeats most optimizations; we have to stop
      // tokenizing here.
      token = pattern;
      pattern = StringPiece();
      container = &parent->recursiveChildren_;
      hasSpecials = true;
    } else {
      token = tokenize(pattern, &hasSpecials);
    }

    auto node = lookupToken(container, token);
    if (!node) {
      container->emplace_back(std::make_unique<GlobNode>(token, hasSpecials));
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

Future<unordered_set<RelativePath>> GlobNode::evaluate(
    RelativePathPiece rootPath,
    TreeInodePtr root) {
  unordered_set<RelativePath> results =
      evaluateRecursiveComponent(rootPath, root).get();
  vector<std::pair<PathComponent, GlobNode*>> recurse;

  {
    auto contents = root->getContents().rlock();
    for (auto& node : children_) {
      if (!node->hasSpecials_) {
        // We can try a lookup for the exact name
        auto it = contents->entries.find(PathComponentPiece(node->pattern_));
        if (it != contents->entries.end()) {
          // Matched!
          if (node->isLeaf_) {
            results.emplace((rootPath + it->first));
            continue;
          }

          // Not the leaf of a pattern; if this is a dir, we need to recurse
          if (it->second.isDirectory()) {
            recurse.emplace_back(std::make_pair(it->first, node.get()));
          }
        }
      } else {
        // We need to match it out of the entries in this inode
        for (auto& entry : contents->entries) {
          if (node->alwaysMatch_ ||
              node->matcher_.match(entry.first.stringPiece())) {
            if (node->isLeaf_) {
              results.emplace((rootPath + entry.first));
              continue;
            }
            // Not the leaf of a pattern; if this is a dir, we need to
            // recurse
            if (entry.second.isDirectory()) {
              recurse.emplace_back(std::make_pair(entry.first, node.get()));
            }
          }
        }
      }
    }
  }

  // Recursively load child inodes and evaluate matches

  std::vector<Future<unordered_set<RelativePath>>> futures;
  for (auto& item : recurse) {
    auto candidateName = rootPath + item.first;
    futures.emplace_back(
        root->getOrLoadChildTree(item.first)
            .then([candidateName, node = item.second](TreeInodePtr dir) {
              return node->evaluate(candidateName, dir);
            }));
  }
  return folly::collect(futures).then(
      [results = std::move(results)](
          std::vector<std::unordered_set<RelativePath>>&& matchVector) mutable {
        for (auto& matches : matchVector) {
          results.insert(matches.begin(), matches.end());
        }
        return results;
      });
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

Future<unordered_set<RelativePath>> GlobNode::evaluateRecursiveComponent(
    RelativePathPiece rootPath,
    TreeInodePtr root) {
  unordered_set<RelativePath> results;
  if (recursiveChildren_.empty()) {
    return results;
  }

  vector<RelativePath> subDirNames;
  {
    auto contents = root->getContents().rlock();
    for (auto& entry : contents->entries) {
      auto candidateName = rootPath + entry.first;

      for (auto& node : recursiveChildren_) {
        if (node->alwaysMatch_ ||
            node->matcher_.match(candidateName.stringPiece())) {
          results.emplace(candidateName);
          // No sense running multiple matches for this same file.
          break;
        }
      }

      // Remember to recurse through child dirs after we've released
      // the lock on the contents.
      if (entry.second.isDirectory()) {
        subDirNames.emplace_back(candidateName);
      }
    }
  }

  // Recursively load child inodes and evaluate matches
  std::vector<Future<unordered_set<RelativePath>>> futures;

  for (auto& candidateName : subDirNames) {
    futures.emplace_back(root->getOrLoadChildTree(candidateName.basename())
                             .then([candidateName, this](TreeInodePtr dir) {
                               return evaluateRecursiveComponent(
                                   candidateName, dir);
                             }));
  }

  return folly::collect(futures).then(
      [results = std::move(results)](
          std::vector<std::unordered_set<RelativePath>>&& matchVector) mutable {
        for (auto& matches : matchVector) {
          results.insert(matches.begin(), matches.end());
        }
        return results;
      });
}
} // namespace eden
} // namespace facebook
