/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "GitIgnorePattern.h"

using folly::StringPiece;
using std::optional;

namespace facebook {
namespace eden {

optional<GitIgnorePattern> GitIgnorePattern::parseLine(StringPiece line) {
  uint32_t flags = 0;

  // Ignore empty lines
  if (line.empty()) {
    return std::nullopt;
  }

  // Lines that start with '#' are ignored as comments.
  // (Whitespace is still relevant though.  The line " #foo" is still parsed
  // and excludes files named " #foo".)
  if (line[0] == '#') {
    return std::nullopt;
  }

  // Lines starting with '!' negate the pattern, and cause the file to be
  // explicitly included even if it matched prior exclude patterns from the
  // same file.
  if (line[0] == '!') {
    flags |= FLAG_INCLUDE;
    // Skip over the '!'
    line.advance(1);
    if (line.empty()) {
      return std::nullopt;
    }
  }

  // If the line ends with "\r\n" rather than just "\n", ignore the "\r"
  if (line.back() == '\r') {
    line.subtract(1);
    if (line.empty()) {
      return std::nullopt;
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
    return std::nullopt;
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
      return std::nullopt;
    }

    // If the pattern happens to end in multiple trailing slashes just ignore
    // it.  git only strips off a single trailing slash.  Patterns that end in
    // multiple trailing slashes can't ever match anything.
    if (line.back() == '/') {
      return std::nullopt;
    }
  }

  // Check to see if the pattern includes any slashes.
  // - If so, we match it against the full path to the file from this
  //   gitignore's directory, using FNM_PATHNAME.
  // - If not, we match it only against the file's base name.
  //
  // Note that this check is done after stripping of any trailing backslash
  // above.
  auto firstSlash = line.find('/');
  if (firstSlash == StringPiece::npos) {
    flags |= FLAG_BASENAME_ONLY;
  } else if (firstSlash == 0) {
    // Skip past this first slash.
    // It only serves to make sure we perform the match against the full path
    // rather than just the basename.
    line.advance(1);
    if (line.empty()) {
      // This probably shouldn't happen since we would have handled it as a
      // trailing slash above.
      return std::nullopt;
    }

    // Patterns starting with two leading slashes can't ever match anything.
    // (git only strips off one slash before using the pattern)
    if (line[0] == '/') {
      return std::nullopt;
    }
  } else if (firstSlash == 2 && line[0] == '*' && line[1] == '*') {
    // As an optimization, if the pattern starts with "**/" and contains no
    // other slashes, just drop the leading "**/" and set FLAG_BASENAME_ONLY.
    //
    // This translates patterns like "**/foo" into just "foo", and "**/*.txt"
    // into "*.txt"
    //
    // In practice, the majority of our ignore patterns using "**" are of this
    // form.
    if (line.find('/', 3) == StringPiece::npos) {
      line.advance(3);
      flags |= FLAG_BASENAME_ONLY;
    }
  }

  // Create the GlobMatcher. Note in gitignore(5), a '**' should include path
  // components that start with '.', so we do not enable the IGNORE_DOTFILES
  // option.
  auto matcher = GlobMatcher::create(line, GlobOptions::DEFAULT);
  if (!matcher.hasValue()) {
    return std::nullopt;
  }

  return GitIgnorePattern(flags, std::move(matcher).value());
}

GitIgnorePattern::GitIgnorePattern(uint32_t flags, GlobMatcher&& matcher)
    : flags_(flags), matcher_(std::move(matcher)) {}

GitIgnorePattern::~GitIgnorePattern() {}

GitIgnore::MatchResult GitIgnorePattern::match(
    RelativePathPiece path,
    PathComponentPiece basename,
    GitIgnore::FileType fileType) const {
  if ((flags_ & FLAG_MUST_BE_DIR) && (fileType != GitIgnore::TYPE_DIR)) {
    return GitIgnore::NO_MATCH;
  }

  bool isMatch = false;
  if (flags_ & FLAG_BASENAME_ONLY) {
    // Match only on the file basename.
    isMatch = matcher_.match(basename.stringPiece());
  } else {
    // Match on full relative path to the file from this directory.
    isMatch = matcher_.match(path.stringPiece());
  }

  if (isMatch) {
    return (flags_ & FLAG_INCLUDE) ? GitIgnore::INCLUDE : GitIgnore::EXCLUDE;
  }

  return GitIgnore::NO_MATCH;
}
} // namespace eden
} // namespace facebook
