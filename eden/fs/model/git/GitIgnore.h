/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <folly/Range.h>
#include <vector>
#include "eden/utils/PathFuncs.h"

namespace facebook {
namespace eden {

class GitIgnorePattern;

/**
 * A GitIgnore object represents the contents of a single .gitignore file
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
 */
class GitIgnore {
 public:
  enum MatchResult {
    EXCLUDE,
    INCLUDE,
    NO_MATCH,
  };

  GitIgnore();
  virtual ~GitIgnore();
  GitIgnore(GitIgnore&&) = default;

  /**
   * Move assignment operator.
   *
   * Note that this operator is not thread safe.  Callers are responsible for
   * providing synchronization between this operation and anyone else using the
   * GitIgnore object from other threads.
   */
  GitIgnore& operator=(GitIgnore&&) = default;

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
   * Check to see if a patch matches any patterns in this GitIgnore object.
   *
   * The input path should be relative to the directory where this .gitignore
   * file exists.  (For repository-wide .gitignore files or for user's personal
   * .gitignore files the path should be relative to the root of the
   * repository.)
   *
   * It is safe to call match() from multiple threads concurrently on the same
   * GitIgnore object, provide no modifying operations are being done to the
   * GitIgnore object at the same time.
   */
  MatchResult match(RelativePathPiece path) const;

  /**
   * Get a human-readable description of a MatchResult enum value.
   *
   * This is mostly for testing and logging.
   */
  static std::string matchString(MatchResult result);

 private:
  GitIgnore(GitIgnore const&) = delete;
  GitIgnore& operator=(GitIgnore const&) = delete;

  /*
   * The patterns loaded from the gitignore file.  These are sorted from
   * highest to lowest precedence (the reverse of the order they are actually
   * listed in the .gitignore file).
   */
  std::vector<GitIgnorePattern> rules_;
};
}
}
