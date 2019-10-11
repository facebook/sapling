/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>
#include <vector>
#include "eden/fs/utils/PathFuncs.h"

namespace facebook {
namespace eden {

class GitIgnorePattern;

/**
 * A GitIgnore object represents the contents of a single .gitignore file
 *
 * Note: Most callers typically do not want to use GitIgnore directly, and
 * should use GitIgnoreStack instead.  The GitIgnoreStack class performs proper
 * checking of the full path against a stack of GitIgnore objects.
 * GitIgnoreStack correctly implements the logic described below:
 *
 * To determine if a path should be included or excluded, you normally must
 * search through multiple separate GitIgnore objects.  These should be
 * processed in the following order (from highest precedence to lowest):
 *
 * - The .gitignore file in the directory containing the path.
 * - The .gitignore file in each subsequent parent directory, all the way up to
 *   the root of the repository.
 * - Any eden client-wide exclude file
 * - The user's personal exclude file.
 *
 * At each step, the GitIgnore object may return that the path was explicitly
 * excluded, explicitly included, or was not matched.  If the path was
 * explicitly excluded or included, stop and use that result.  Otherwise
 * proceed to the next highest precedence GitIgnore object.
 *
 * If a directory is ignored, everything inside it is always ignored.  Files
 * inside this directory may be explicitly added to source control, but
 * untracked files inside this directory (and any children directories) are
 * always ignored: explicit include rules cannot be used to unignore files
 * inside an ignored directory.
 */
class GitIgnore {
 public:
  enum MatchResult {
    /**
     * This path is explicitly excluded by the ignore rules.
     */
    EXCLUDE,
    /**
     * This path is explicitly included by the ignore rules.
     */
    INCLUDE,
    /**
     * This path does not match any ignore rules.
     * Processing may continue to the next GitIgnore object.  If no rules match
     * in any GitIgnore object, the path is implicitly included.
     */
    NO_MATCH,
    /**
     * This is a special hidden path.
     * It should not be reported at all, not even as an ignored file.
     * This is used for special directories like .hg and .eden
     */
    HIDDEN,
  };

  /**
   * An enum to indicate if the input path refers to a directory or not.
   *
   * This is required for matching since gitignore patterns ending in a
   * trailing slash only match directories.
   *
   * The enum values use a "TYPE_" prefix to avoid any potential confusion with
   * the "FILE" type name from <stdio.h> or the "DIR" typename from <dirent.h>
   */
  enum FileType {
    /**
     * A regular file, or a symbolic link.
     */
    TYPE_FILE,
    /**
     * A directory.
     */
    TYPE_DIR,
  };

  GitIgnore();
  virtual ~GitIgnore();
  GitIgnore(GitIgnore&&) noexcept(
      std::is_nothrow_move_constructible<std::vector<GitIgnorePattern>>::value);
  GitIgnore(GitIgnore const&);
  GitIgnore& operator=(GitIgnore const&);

  /**
   * Move assignment operator.
   *
   * Note that this operator is not thread safe.  Callers are responsible for
   * providing synchronization between this operation and anyone else using the
   * GitIgnore object from other threads.
   */
  GitIgnore& operator=(GitIgnore&&) noexcept(
      std::is_nothrow_move_assignable<std::vector<GitIgnorePattern>>::value);

  /**
   * Parse the contents of a gitignore file.
   *
   * Generally you should call this method exactly once immediately
   * after constructing a GitIgnore object.
   *
   * If loadFile() is called more than once, subsequent calls replace the
   * contents loaded by previous calls.
   *
   * loadFile() is not thread safe.  Callers are responsible for providing
   * synchronization between loadFile() and match() operations done in multiple
   * threads.
   */
  void loadFile(folly::StringPiece contents);

  /**
   * Check to see if a path matches any patterns in this GitIgnore object.
   *
   * The input path should be relative to the directory where this .gitignore
   * file exists.  (For repository-wide .gitignore files or for user's personal
   * .gitignore files the path should be relative to the root of the
   * repository.)
   *
   * The fileType argument indicates if this path refers to a directory or not.
   * This is required since ignore patterns ending with a trailing slash match
   * directories only.
   *
   * It is safe to call match() from multiple threads concurrently on the same
   * GitIgnore object, provide no modifying operations are being done to the
   * GitIgnore object at the same time.
   */
  MatchResult match(RelativePathPiece path, FileType fileType) const {
    return match(path, path.basename(), fileType);
  }

  /**
   * A version of match() that accepts both the path and the basename.
   *
   * The path parameter should still include the basename (it should not be
   * just the dirname component).
   *
   * While match() could just compute the basename on its own, many patterns
   * require the basename, and code checking the ignore status for a path
   * generally checks the path against many patterns across several GitIgnore
   * files.  It is slightly more efficient for the caller to compute the
   * basename once, rather than re-computing it for each pattern that needs it.
   */
  MatchResult match(
      RelativePathPiece path,
      PathComponentPiece basename,
      FileType fileType) const;

  /**
   * @return true if there are no rules.
   */
  bool empty() const {
    return rules_.empty();
  }

  /**
   * Get a human-readable description of a MatchResult enum value.
   *
   * This is mostly for testing and logging.
   */
  static std::string matchString(MatchResult result);

 private:
  /*
   * The patterns loaded from the gitignore file.  These are sorted from
   * highest to lowest precedence (the reverse of the order they are actually
   * listed in the .gitignore file).
   */
  std::vector<GitIgnorePattern> rules_;
};
} // namespace eden
} // namespace facebook
