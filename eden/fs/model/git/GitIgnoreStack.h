/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include <string>
#include "eden/fs/model/git/GitIgnore.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook {
namespace eden {

/**
 * GitIgnoreStack represents a stack of GitIgnore files, one per directory
 * level.
 *
 * This provides an API for checking the ignore status of paths inside a
 * directory.  The path name will be checked against the ignore rules found in
 * its directory, and if no match is found, the ignore rules from its parent
 * directory, and so on all the way up to the root.
 *
 * Several notes about usage:
 * - GitIgnoreStack objects are really just nodes in the stack.  They contain a
 *   pointer to their parent GitIgnoreStack node.
 *
 * - GitIgnoreStack objects refer to their parent with a raw pointer, and rely
 *   on the user to ensure that parent GitIgnoreStack objects always exist for
 *   longer than children GitIgnoreStacks that refer to them.  (We could have
 *   used a shared_ptr to ensure that parents exist for as long as the
 *   children, but in practice state for the parent directory always outlives
 *   state for the children directories anyway, so there is no real need to
 *   track ownership inside GitIgnoreStack using shared_ptr refcounts.)
 *
 * - You must create a GitIgnoreStack object for each directory, even if that
 *   directory does not contain a .gitignore file.  gitignore rules are always
 *   relative to the directory that contains the .gitignore file.  We use the
 *   number of levels in the GitIgnoreStack to figure out which part of the
 *   path the rules apply to.
 */
class GitIgnoreStack {
 public:
  /**
   * Create a new GitIgnoreStack for a directory that does not contain a
   * .gitignore file.
   */
  explicit GitIgnoreStack(const GitIgnoreStack* parent) : parent_{parent} {}

  /**
   * Create a new GitIgnoreStack for a directory that contains a .gitignore
   * file.
   */
  GitIgnoreStack(
      const GitIgnoreStack* parent,
      folly::StringPiece ignoreFileContents)
      : parent_{parent} {
    ignore_.loadFile(ignoreFileContents);
  }

  GitIgnoreStack(const GitIgnoreStack* parent, GitIgnore ignore)
      : ignore_{std::move(ignore)}, parent_{parent} {}

  /**
   * Get the MatchResult for a path.
   */
  GitIgnore::MatchResult match(
      RelativePathPiece path,
      GitIgnore::FileType fileType) const;

  bool empty() const {
    return ignore_.empty();
  }

 private:
  /**
   * The GitIgnore info for this node on the stack
   */
  GitIgnore ignore_;

  /**
   * A pointer to the next node in the stack.
   * This will be the GitIgnore data for the next ancestor directory
   * that contains a .gitignore file.  This will be nullptr if no other
   * ancestor directories contain a .gitignore file.
   *
   * This is a non-owning pointer.  Our caller is responsible for ensuring that
   * parent nodes on the stack always live longer than children nodes.
   */
  const GitIgnoreStack* parent_{nullptr};
};
} // namespace eden
} // namespace facebook
