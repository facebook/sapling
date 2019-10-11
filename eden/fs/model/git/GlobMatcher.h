/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Expected.h>
#include <folly/Range.h>
#include <cstdint>
#include <vector>

namespace facebook {
namespace eden {

/**
 * Options type for GlobMatcher::create(). Multiple values can be OR'd together.
 * DEFAULT should be used to signal no options should be enabled.
 */
enum class GlobOptions : uint32_t {
  DEFAULT = 0x00,
  IGNORE_DOTFILES = 0x01,
};

GlobOptions operator|(GlobOptions a, GlobOptions b);
GlobOptions& operator|=(GlobOptions& a, GlobOptions b);
bool operator&(GlobOptions a, GlobOptions b);

/**
 * GlobMatcher performs matching of filename glob patterns.
 *
 * This aims to be 100% compatible with the syntax used in gitignore files.
 *
 * This code is optimized for loading glob patterns once, and then repeatedly
 * matching on them.  It does some basic pre-processing of the glob pattern,
 * allowing it to perform matches more efficiently.  (In basic benchmarks I
 * have run it ranges from 50% to 100% faster than the wildmatch()
 * implementation used by git, depending on the pattern.)
 */
class GlobMatcher {
 public:
  /**
   * Default constructor for GlobMatcher.
   *
   * This will create a GlobMatcher that only matches the empty string.
   * Use GlobMatcher::create() to initialize a normal glob matcher.  The
   * default constructor is provided primarily if you want to initialize the
   * object later by move-assigning it from the result of create().
   */
  GlobMatcher();
  ~GlobMatcher();
  GlobMatcher(GlobMatcher&&) = default;
  GlobMatcher& operator=(GlobMatcher&&) = default;
  GlobMatcher(const GlobMatcher&) = default;
  GlobMatcher& operator=(const GlobMatcher&) = default;

  /**
   * Create a GlobMatcher object from a glob pattern.
   *
   * Returns a GlobMatcher, or a string describing why the glob pattern was
   * invalid.  This function may also throw std::bad_alloc if memory allocation
   * fails.
   */
  static folly::Expected<GlobMatcher, std::string> create(
      folly::StringPiece glob,
      GlobOptions options);

  /**
   * Match a string against this glob pattern.
   *
   * Returns true if the text matches the pattern, or false otherwise.
   * The entire text must match the pattern.  (If a only substring matches the
   * pattern this method will still return false.)
   */
  bool match(folly::StringPiece text) const;

 private:
  explicit GlobMatcher(std::vector<uint8_t> pattern);

  static folly::Expected<size_t, std::string> parseBracketExpr(
      folly::StringPiece glob,
      size_t idx,
      std::vector<uint8_t>* pattern);
  static bool addCharClass(
      folly::StringPiece charClass,
      std::vector<uint8_t>* pattern);

  /**
   * Returns true if the trailing section of the input text (starting at
   * textIdx) is a mattern for the trailing portion of the pattern buffer
   * (starting at patternIdx).
   */
  bool tryMatchAt(folly::StringPiece text, size_t textIdx, size_t patternIdx)
      const;

  /**
   * Check to see if the given character matches the character class opcode
   * starting at the specified index in the pattern buffer.
   *
   * Returns true if the character matches, and false otherwise.
   *
   * The patternIdx argument is updated to point to the next opcode after this
   * character calss.
   */
  bool charClassMatch(uint8_t ch, size_t* patternIdx) const;

  /**
   * pattern_ is a pre-processed version of the glob pattern.
   *
   * This consists of a list of opcodes.
   *
   * TODO: It's perhaps worth doing some small-string optimization here.
   * In practice, over 90% of our gitignore patterns are less than 24 bytes.
   * It would probably be better to just store them inline here in this case,
   * rather than heap-allocating them in a vector.
   */
  std::vector<uint8_t> pattern_;
};
} // namespace eden
} // namespace facebook
