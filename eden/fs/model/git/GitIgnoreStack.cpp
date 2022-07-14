/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/model/git/GitIgnoreStack.h"

namespace facebook::eden {

namespace {
constexpr static PathComponentPiece kHgName{".hg"};
constexpr static PathComponentPiece kEdenName{".eden"};
} // namespace

GitIgnore::MatchResult GitIgnoreStack::match(
    RelativePathPiece path,
    GitIgnore::FileType fileType) const {
  // Explicitly hide any entry named .hg or .eden
  //
  // We only check the very last component of the path.  Since these
  // directories are hidden the status code generally should not descend into
  // them and have to check ignore status for path names inside these
  // directories.
  const auto basename = path.basename();
  if (basename == kHgName || basename == kEdenName) {
    return GitIgnore::HIDDEN;
  }

  // Walk upwards through the GitIgnore stack, checking the path relative to
  // each directory against the GitIgnore rules for that directory.
  const auto* node = this;
  const auto suffixRange = path.rsuffixes();
  auto suffixIter = suffixRange.begin();
  while (node != nullptr) {
    RelativePathPiece suffix;
    if (suffixIter == suffixRange.end()) {
      // There may still be GitIgnore nodes to check even once we reach the
      // root directory.  The very first nodes in the ignore stack are used for
      // user-specific ignore rules, system-wide ignore rules, etc.
      //
      // All of these match against the full path from the mount point root.
      suffix = path;
    } else {
      suffix = *suffixIter;
      ++suffixIter;
    }

    const GitIgnore* ignore = &node->ignore_;
    node = node->parent_;

    const auto result = ignore->match(suffix, basename, fileType);
    if (result != GitIgnore::NO_MATCH) {
      return result;
    }
  }
  return GitIgnore::NO_MATCH;
}

} // namespace facebook::eden
