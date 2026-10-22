/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/model/git/GitIgnorePattern.h"

#include <string>

using folly::StringPiece;
using std::optional;

namespace facebook::eden {

namespace {

size_t skipBracketExpression(StringPiece pattern, size_t openIdx) {
  size_t idx = openIdx + 1;
  if (idx < pattern.size() && (pattern[idx] == '!' || pattern[idx] == '^')) {
    ++idx;
  }
  if (idx < pattern.size() && pattern[idx] == ']') {
    ++idx;
  }
  while (idx < pattern.size() && pattern[idx] != ']') {
    if (pattern[idx] == '\\' && idx + 1 < pattern.size()) {
      ++idx;
    }
    ++idx;
  }
  return idx < pattern.size() ? idx + 1 : openIdx + 1;
}

std::string normalizeStarRuns(StringPiece pattern) {
  std::string result;
  result.reserve(pattern.size());
  size_t idx = 0;
  while (idx < pattern.size()) {
    char c = pattern[idx];
    if (c == '\\' && idx + 1 < pattern.size()) {
      result.append(pattern.data() + idx, 2);
      idx += 2;
    } else if (c == '[') {
      size_t end = skipBracketExpression(pattern, idx);
      result.append(pattern.data() + idx, end - idx);
      idx = end;
    } else if (c == '*') {
      size_t runEnd = idx;
      while (runEnd < pattern.size() && pattern[runEnd] == '*') {
        ++runEnd;
      }
      bool atComponentStart = idx == 0 || pattern[idx - 1] == '/';
      bool atComponentEnd = runEnd == pattern.size() || pattern[runEnd] == '/';
      if (runEnd - idx >= 2 && atComponentStart && atComponentEnd) {
        result.append("**");
      } else {
        result.push_back('*');
      }
      idx = runEnd;
    } else {
      result.push_back(c);
      ++idx;
    }
  }
  return result;
}

} // namespace

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
  size_t end = line.size();
  while (end > 0 && line[end - 1] == ' ') {
    size_t precedingBackslashes = 0;
    while (precedingBackslashes + 1 < end &&
           line[end - 2 - precedingBackslashes] == '\\') {
      ++precedingBackslashes;
    }
    if (precedingBackslashes % 2 == 1) {
      // This space is backslash escaped, so stop here, and include
      // it in the pattern.
      break;
    }
    // Ignore this unescaped trailing space.
    --end;
  }
  line.subtract(line.size() - end);
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

  auto normalized = normalizeStarRuns(line);
  if (normalized == "**") {
    normalized = "*";
    flags |= FLAG_BASENAME_ONLY;
  }

  // Create the GlobMatcher. Note in gitignore(5), a '**' should include path
  // components that start with '.', so we do not enable the IGNORE_DOTFILES
  // option.
  auto matcher = GlobMatcher::create(normalized, GlobOptions::DEFAULT);
  if (!matcher.hasValue()) {
    return std::nullopt;
  }

  return GitIgnorePattern(flags, std::move(matcher).value());
}

GitIgnorePattern::GitIgnorePattern(uint32_t flags, GlobMatcher&& matcher)
    : flags_(flags), matcher_(std::move(matcher)) {}

GitIgnorePattern::~GitIgnorePattern() = default;

GitIgnore::MatchResult GitIgnorePattern::match(
    RelativePathPiece path,
    PathComponentPiece basename,
    GitIgnore::FileType fileType,
    const GlobMatchOptions& matchOptions) const {
  if ((flags_ & FLAG_MUST_BE_DIR) && (fileType != GitIgnore::TYPE_DIR)) {
    return GitIgnore::NO_MATCH;
  }

  bool isMatch = false;
  if (flags_ & FLAG_BASENAME_ONLY) {
    // Match only on the file basename.
    isMatch = matcher_.match(basename.view(), matchOptions);
  } else {
    // Match on full relative path to the file from this directory.
    isMatch = matcher_.match(path.view(), matchOptions);
  }

  if (isMatch) {
    return (flags_ & FLAG_INCLUDE) ? GitIgnore::INCLUDE : GitIgnore::EXCLUDE;
  }

  return GitIgnore::NO_MATCH;
}

} // namespace facebook::eden
