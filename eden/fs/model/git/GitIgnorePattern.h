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

#include <folly/Optional.h>
#include <folly/Range.h>
#include "eden/fs/model/git/GitIgnore.h"

namespace facebook {
namespace eden {

/**
 * A single pattern loaded from a .gitignore file.
 *
 * Each line in a .gitignore file is converted into a separate GitIgnorePattern
 * object.  (Except for empty lines, comments, or otherwise invalid lines,
 * which don't result in any GitIgnorePattern.)
 */
class GitIgnorePattern {
 public:
  /**
   * Parse a line from a gitignore file.
   *
   * Returns a GitIgnorePattern, or folly::none if the line did not contain a
   * pattern (e.g., if it was empty or a comment).
   */
  static folly::Optional<GitIgnorePattern> parseLine(folly::StringPiece line);

  virtual ~GitIgnorePattern();
  GitIgnorePattern(GitIgnorePattern&&) = default;
  GitIgnorePattern& operator=(GitIgnorePattern&&) = default;

  /**
   * Check to see if a pathname matches this pattern.
   *
   * The pathname should be relative to the directory where this pattern was
   * loaded from.  For example, if this pattern was loaded from
   * <repo_root>/foo/bar/.gitignore, when testing the file
   * <repo_root>/foo/bar/abc/xyz.txt, pass in the path as "abc/xyz.txt"
   */
  GitIgnore::MatchResult match(RelativePathPiece path) const;

 private:
  // Flag values that can be bitwise-ORed to create the flags_ value.
  enum Flags : uint32_t {
    // This pattern started with !, indicating we should explicitly include
    // the anything matching it.
    FLAG_INCLUDE = 0x01,
    // The pattern ended with /, indicating it should only match directories.
    FLAG_MUST_BE_DIR = 0x02,
    // The pattern did not contain /, so it only matches against the last
    // component of any path.
    FLAG_BASENAME_ONLY = 0x04,
  };

  GitIgnorePattern(uint32_t flags, folly::StringPiece pattern);

  GitIgnorePattern(GitIgnorePattern const&) = delete;
  GitIgnorePattern& operator=(GitIgnorePattern const&) = delete;

  bool fnmatch(folly::StringPiece value) const;

  /**
   * Whether this is an include or exclude pattern.
   */
  uint32_t flags_{0};
  std::string pattern_;
};
}
}
