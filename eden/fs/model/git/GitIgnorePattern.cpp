/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "GitIgnorePattern.h"

#include <fnmatch.h>

using folly::Optional;
using folly::StringPiece;

namespace facebook {
namespace eden {

Optional<GitIgnorePattern> GitIgnorePattern::parseLine(StringPiece line) {
  uint32_t flags = 0;

  // Ignore empty lines
  if (line.empty()) {
    return folly::none;
  }

  // Lines that start with '#' are ignored as comments.
  // (Whitespace is still relevant though.  The line " #foo" is still parsed
  // and excludes files named " #foo".)
  if (line[0] == '#') {
    return folly::none;
  }

  // Lines starting with '!' negate the pattern, and cause the file to be
  // explicitly included even if it matched prior exclude patterns from the
  // same file.
  if (line[0] == '!') {
    flags |= FLAG_INCLUDE;
    // Skip over the '!'
    line.advance(1);
    if (line.empty()) {
      return folly::none;
    }
  }

  // If the line ends with "\r\n" rather than just "\n", ignore the "\r"
  if (line.back() == '\r') {
    line.subtract(1);
    if (line.empty()) {
      return folly::none;
    }
  }

  // Trim all unescaped trailing spaces.
  const char* pos = line.end();
  while (pos > line.begin()) {
    if (*(pos - 1) != ' ') {
      break;
    }
    if ((pos - 2) >= line.begin() && *(pos - 2) == '\\') {
      // This space is backslash escaped, so stop here, and include
      // it in the pattern.
      break;
    }
    // Ignore this unescaped trailing space.
    --pos;
  }
  line.assign(line.begin(), pos);
  if (line.empty()) {
    return folly::none;
  }

  // If the pattern ends with a trailing slash then it only matches
  // directories.  We drop the trailing slash from the pattern though,
  // since the paths we match against won't actually include a trailing slash.
  if (line.back() == '/') {
    flags |= FLAG_MUST_BE_DIR;
    line.subtract(1);

    // If '/' was the only character in the pattern it looks like git just
    // ignores the pattern (as opposed to ignoring everything in the
    // directory).
    if (line.empty()) {
      return folly::none;
    }

    // If the pattern happens to end in multiple trailing slashes just ignore
    // it.  git only strips off a single trailing slash.  Patterns that end in
    // multiple trailing slashes can't ever match anything.
    if (line.back() == '/') {
      return folly::none;
    }
  }

  // Check to see if the pattern includes any slashes.
  // - If so, we match it against the full path to the file from this
  //   gitignore's directory, using FNM_PATHNAME.
  // - If not, we match it only against the file's base name.
  //
  // Note that this check is done after stripping of any trailing backslash
  // above.
  ssize_t firstSlash = -1;
  for (size_t idx = 0; idx < line.size(); ++idx) {
    if (line[idx] == '/') {
      firstSlash = idx;
      break;
    }
  }
  if (firstSlash < 0) {
    flags |= FLAG_BASENAME_ONLY;
  } else if (firstSlash == 0) {
    // Skip past this first slash.
    // It only serves to make sure we perform the match against the full path
    // rather than just the basename.
    line.advance(1);
    if (line.empty()) {
      // This probably shouldn't happen since we would have handled it as a
      // trailing slash above.
      return folly::none;
    }

    // Patterns starting with two leading slashes can't ever match anything.
    // (git only strips off one slash before using the pattern)
    if (line[0] == '/') {
      return folly::none;
    }
  }

  // TODO: git also tracks how much of the leading portion of the pattern does
  // not contain any glob-special characters, so they can do fixed-string
  // matching against that portion.

  // TODO: git also has a EXC_FLAG_ENDSWITH flag they use to optimize matching
  // of patterns like "*.txt", ".c", etc.  (initial wildcard followed by all
  // non-wildcard, non-slash characters).

  // TODO
  //
  // "**" is special:
  // - leading "**/" matches any leading directory
  // - trailing "/**" matches any trailing suffix
  // - "/**/" matches zero or more directories
  // - other consecutive asterisks are invalid

  return GitIgnorePattern(flags, line);
}

GitIgnorePattern::GitIgnorePattern(uint32_t flags, StringPiece pattern)
    : flags_(flags), pattern_(pattern.str()) {}

GitIgnorePattern::~GitIgnorePattern() {}

GitIgnore::MatchResult GitIgnorePattern::match(RelativePathPiece path) const {
  if (flags_ & FLAG_MUST_BE_DIR) {
    // TODO: get the file type, and reject the file if it's not a directory.
    // git does this lazily-ish.  It may or may not know the entry type ahead
    // of time, and only looks it up if FLAG_MUST_BE_DIR is set.
  }

  bool isMatch = false;
  if (flags_ & FLAG_BASENAME_ONLY) {
    // Match only on the file basename.
    isMatch = fnmatch(path.basename().stringPiece());
  } else {
    // Match on full relative path to the file from this directory.
    isMatch = fnmatch(path.stringPiece());
  }

  if (isMatch) {
    return (flags_ & FLAG_INCLUDE) ? GitIgnore::INCLUDE : GitIgnore::EXCLUDE;
  }

  return GitIgnore::NO_MATCH;
}

bool GitIgnorePattern::fnmatch(folly::StringPiece value) const {
  // FIXME: We're just using the standard fnmatch() function for now.
  // This has several issues:
  // - I don't believe it will handle "**" correctly
  // - It's going to hurt performance a lot that we have to call value.str()
  //   and allocate a new string each time we perform matching.
  // - I suspect this function may not exist on all platforms we care about.
  //
  // Both git and libgit2 appear to have their own custom implementations here.
  // We'll probably need to do the same.  (It doesn't look like the version in
  // libgit2 is exposed in a way that we can call it.  Even if we could, it
  // wouldn't handle our non-null-terminated StringPiece objects.)
  //
  // git also performs additional optimizations that we probably will want to
  // do as well:
  // - special handling for "endswith" type patterns ("*.txt", "*.c", etc)
  // - special handling for fixed strings, and for just the leading fixed
  //   portion of a pattern.
  int rc = ::fnmatch(pattern_.c_str(), value.str().c_str(), FNM_PATHNAME);
  return rc == 0;
}
}
}
