/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/model/git/GlobMatcher.h"

#include <folly/logging/xlog.h>

using folly::ByteRange;
using folly::Expected;
using folly::StringPiece;
using std::string;
using std::vector;

namespace {
/*
 * Opcode characters for our pattern buffer.
 */
enum : uint8_t {
  // A chunk of literal string data.
  // This is followed by a length byte, then the literal data.
  // Literal runs of more than 255 bytes in a row are broken up into separate
  // literal opcodes with a max length of 255 bytes each.
  GLOB_LITERAL = 'S',
  // GLOB_STAR matches 0 or more characters.
  // This is followed by a bool byte. If true, the pattern can match text
  // that starts with a '.'.
  // Any character except '/' can be matched.
  GLOB_STAR = '*',
  // GLOB_STAR_STAR_END matches all remaining text.
  // This is followed by a bool byte. If true, a path component in the pattern
  // can start with a '.'.
  // If GLOB_STAR_STAR_END appears it is always the very last opcode in the
  // pattern buffer.
  GLOB_STAR_STAR_END = '>',
  // GLOB_STAR_STAR_SLASH matches either:
  // - 0 characters
  // - 1 or more characters followed by a slash
  // This is followed by a bool byte. If true, a path component in the pattern
  // can start with a '.'.
  GLOB_STAR_STAR_SLASH = 'X',
  // GLOB_CHAR_CLASS matches a character class.
  // This is followed by a list of characters to match.
  // The matching characters are encoded as follows:
  // - '\x00' indicates the end of the character class
  // - '\x01' indicates a range.  It is followed by 2 bytes, the low and high
  //    bounds of the range (inclusive).
  // - any other character matches only that character.
  // A literal '\x00' or '\x01' is encoded as a range with itself as both the
  // lower and upper bound.  e.g. '\x00' gets encoded as '\x01\x00\x00'.
  GLOB_CHAR_CLASS = '[',
  // GLOB_CHAR_CLASS_NEGATED is like GLOB_CHAR_CLASS, but matches
  // only if the character does not match the character class.
  // TODO: Do not let a negated character class pattern match a "." at the start
  // of a file name, as specified in the POSIX docs.
  GLOB_CHAR_CLASS_NEGATED = ']',
  GLOB_CHAR_CLASS_END = '\x00',
  GLOB_CHAR_CLASS_RANGE = '\x01',
  // GLOB_QMARK matches any single character except for '/'
  GLOB_QMARK = '?',
  // GLOB_ENDS_WITH matches a literal section at the end of the string.
  // We optimize GLOB_STAR+GLOB_LITERAL at the end of the pattern into
  // GLOB_ENDS_WITH, so it is composed of the bool byte from GLOB_STAR followed
  // by the data from GLOB_LITERAL.
  GLOB_ENDS_WITH = '$',
  // Used to represent boolean values associated with an opcode.
  GLOB_TRUE = 'T',
  GLOB_FALSE = 'F',
};

} // namespace

namespace facebook {
namespace eden {

GlobOptions operator|(GlobOptions a, GlobOptions b) {
  return static_cast<GlobOptions>(
      static_cast<uint32_t>(a) | static_cast<uint32_t>(b));
}

GlobOptions& operator|=(GlobOptions& a, GlobOptions b) {
  a = (a | b);
  return a;
}

bool operator&(GlobOptions a, GlobOptions b) {
  return (static_cast<uint32_t>(a) & static_cast<uint32_t>(b)) != 0;
}

GlobMatcher::GlobMatcher(vector<uint8_t> pattern)
    : pattern_(std::move(pattern)) {}

GlobMatcher::GlobMatcher() {}

GlobMatcher::~GlobMatcher() {}

/*
 * A glob pattern consists of a few types of data:
 * - literal string pieces
 * - *
 * - **
 * - ?
 * - bracket expressions ([])
 *
 * We parse this in create(), and encode it as a string of opcodes.
 * The opcode semantics are documented above where they are defined.
 *
 * Glancing through our existing ignore rules:
 * - About 60% are simple fixed strings, with no wildcards
 * - About 27% are simple "ends with" patterns (e.g., "*.txt")
 */
Expected<GlobMatcher, string> GlobMatcher::create(
    StringPiece glob,
    GlobOptions options) {
  vector<uint8_t> result;
  // Make a guess at how big the pattern buffer will be.
  // We require 2 extra bytes for each literal chunk.  We save a byte for "**"
  // expressions, and we usually save a byte or two on bracket expressions.
  result.reserve(glob.size() + 6);

  ssize_t prevOpcodeIdx = -1;
  ssize_t curOpcodeIdx = -1;
  auto addOpcode = [&](uint8_t opcode) {
    prevOpcodeIdx = curOpcodeIdx;
    curOpcodeIdx = result.size();
    result.push_back(opcode);
  };

  auto appendLiteralChar = [&](char c) {
    if (curOpcodeIdx >= 0 && result[curOpcodeIdx] == GLOB_LITERAL &&
        result[curOpcodeIdx + 1] < 0xff) {
      // Just append this byte to the end of the current literal section.
      ++result[curOpcodeIdx + 1];
      result.push_back(c);
    } else {
      // We aren't currently in a literal section (or we have already put 255
      // bytes in the current section and can't fit any more).  Start a new
      // literal section.
      addOpcode(GLOB_LITERAL);
      result.push_back(1);
      result.push_back(c);
    }
  };

  auto appendBool = [&](bool b) {
    result.push_back(b ? GLOB_TRUE : GLOB_FALSE);
  };

  // Note: watchman's wildcard matching code treats '/' slightly specially:
  // it can match 1 or more '/' characters.  For example, "foo/bar" would match
  // "foo///bar".
  //
  // We don't bother doing this here since the paths given to our code should
  // already have been normalized, so we should never have repeated slashes in
  // the text being matched.

  auto includeDotfiles = !(options & GlobOptions::IGNORE_DOTFILES);
  for (size_t idx = 0; idx < glob.size(); ++idx) {
    char c = glob[idx];
    if (c == '\\') {
      // Backslash escaped characters are treated literally
      ++idx;
      if (idx >= glob.size()) {
        // A trailing backslash is invalid.  This glob should be ignored.
        return folly::makeUnexpected<string>(
            "glob pattern ends with trailing backslash");
      }
      appendLiteralChar(glob[idx]);
      continue;
    } else if (c == '?') {
      // Match any single character except for a slash
      addOpcode(GLOB_QMARK);
    } else if (c == '*') {
      if (idx + 1 < glob.size() && glob[idx + 1] == '*') {
        // This is "**".
        // According to the gitignore man pages, "**" is only valid in three
        // cases:
        // - "**/" at the start of the pattern
        // - "/**" at the end of the pattern
        // - "/**/" in the middle of the pattern
        ++idx;
        if (idx + 1 >= glob.size()) {
          // Make sure that the character before this was '/'.
          // We still treat it as part of the previous literal opcode, but we
          // want to reject the glob if this ** wasn't preceded by '/'.
          if (idx < 2 || glob[idx - 2] != '/') {
            return folly::makeUnexpected<string>(
                "invalid \"**\" sequence at end of pattern without slash");
          }
          addOpcode(GLOB_STAR_STAR_END);
          appendBool(includeDotfiles);
        } else if (glob[idx + 1] == '/') {
          if (idx >= 2 && glob[idx - 2] != '/') {
            return folly::makeUnexpected<string>(
                "\"**/\" must follow a slash or appear at the start of a pattern");
          }

          ++idx;
          addOpcode(GLOB_STAR_STAR_SLASH);
          appendBool(includeDotfiles);
        } else {
          // Reject the pattern if "**" isn't followed by the end of the
          // pattern or a "/"
          return folly::makeUnexpected<string>("invalid \"**\" sequence");
        }
      } else {
        addOpcode(GLOB_STAR);
        // If includeDotfiles is false, then "*.cpp" should not match
        // ".bak.cpp", but "My*.cpp" should match "My.foo.cpp", so we must check
        // the preceding character.
        appendBool(includeDotfiles || (idx != 0 && glob[idx - 1] != '/'));
      }
    } else if (c == '[') {
      // Translate a bracket expression
      prevOpcodeIdx = curOpcodeIdx;
      curOpcodeIdx = result.size();
      auto newIdx = parseBracketExpr(glob, idx, &result);
      if (!newIdx.hasValue()) {
        return folly::makeUnexpected<string>(std::move(newIdx).error());
      }
      idx = newIdx.value();
    } else {
      appendLiteralChar(c);
    }
  }

  // We perform one additional optimization here:
  // if the final two opcodes were GLOB_STAR followed by GLOB_LITERAL, we
  // translate this into GLOB_ENDS_WITH.
  if (prevOpcodeIdx >= 0 && result[prevOpcodeIdx] == GLOB_STAR &&
      result[curOpcodeIdx] == GLOB_LITERAL) {
    // Currently, the end of the result vector contains:
    //
    // [prevOpcodeIdx] GLOB_STAR
    //                 GLOB_STAR matchCanStartWithDot bool
    // [curOpcodeIdx]  GLOB_LITERAL
    //                 GLOB_LITERAL data
    //
    // We modify it so it becomes:
    //
    // [prevOpcodeIdx] GLOB_ENDS_WITH
    //                 GLOB_STAR matchCanStartWithDot bool
    // [curOpcodeIdx]  GLOB_LITERAL data
    result.erase(result.begin() + curOpcodeIdx);
    result[prevOpcodeIdx] = GLOB_ENDS_WITH;
  }

  return GlobMatcher(std::move(result));
}

Expected<size_t, string> GlobMatcher::parseBracketExpr(
    StringPiece glob,
    size_t idx,
    vector<uint8_t>* pattern) {
  DCHECK_LT(idx, glob.size());
  DCHECK_EQ(glob[idx], '[');

  // Check for a leading '!' or '^'
  if (idx + 1 >= glob.size()) {
    return folly::makeUnexpected<string>("unterminated bracket sequence");
  }
  if (glob[idx + 1] == '!' || glob[idx + 1] == '^') {
    pattern->push_back(GLOB_CHAR_CLASS_NEGATED);
    ++idx;
    if (idx >= glob.size()) {
      return folly::makeUnexpected<string>("unterminated bracket sequence");
    }
  } else {
    pattern->push_back(GLOB_CHAR_CLASS);
  }

  // Set NO_PREV_CHAR to something outside of the range [-128, 255]
  // We want to make sure it can't possibly correspond to a valid char value,
  // regardless of whether char types are signed or unsigned on this platform.
  constexpr int32_t NO_PREV_CHAR = 0xffff;
  int32_t prevChar = NO_PREV_CHAR;
  auto addPrevChar = [&]() {
    if (prevChar == NO_PREV_CHAR) {
      return;
    } else if (
        prevChar == GLOB_CHAR_CLASS_END || prevChar == GLOB_CHAR_CLASS_RANGE) {
      // Escape these characters by turning them into ranges.
      pattern->push_back(GLOB_CHAR_CLASS_RANGE);
      pattern->push_back(prevChar);
      pattern->push_back(prevChar);
    } else {
      pattern->push_back(prevChar);
    }
  };

  auto startIdx = idx;
  while (true) {
    ++idx;
    if (idx >= glob.size()) {
      return folly::makeUnexpected<string>("unterminated bracket sequence");
    }

    auto c = glob[idx];
    if (c == '\\') {
      // A backslash escapes the following character
      ++idx;
      if (idx >= glob.size()) {
        // Unterminated escape sequence
        return folly::makeUnexpected<string>(
            "unterminated backslash in bracket sequence");
      }
      addPrevChar();
      prevChar = glob[idx];
    } else if (c == ']') {
      // ']' normally signifies the end of the character class,
      // unless it is the very first character after the opening '[' or '[^'
      if (idx == startIdx + 1) {
        DCHECK_EQ(NO_PREV_CHAR, prevChar);
        prevChar = c;
      } else {
        // End of the character class.
        break;
      }
    } else if (c == '-') {
      if (prevChar == NO_PREV_CHAR) {
        prevChar = c;
      } else {
        // This is a range
        if (idx + 1 >= glob.size()) {
          // Unterminated escape sequence
          return folly::makeUnexpected<string>("unterminated bracket range");
        } else if (glob[idx + 1] == ']') {
          // '-' followed by the terminating ']' is just a literal '-',
          // not a range.
          addPrevChar();
          prevChar = c;
        } else {
          // This is a range
          ++idx;
          uint8_t highBound = glob[idx];
          if (highBound == '\\') {
            ++idx;
            if (idx >= glob.size()) {
              return folly::makeUnexpected<string>(
                  "unterminated escape in bracket range");
            }
            highBound = glob[idx];
          }
          // Don't even bother adding the range if the low bound is greater
          // than the high bound.  (We don't treat the whole glob as invalid
          // though.  We just ignore this one range, since it can never match
          // anything.)
          if (prevChar <= highBound) {
            pattern->push_back(GLOB_CHAR_CLASS_RANGE);
            pattern->push_back(prevChar);
            pattern->push_back(highBound);
          }
          prevChar = NO_PREV_CHAR;
        }
      }
    } else if (c == '[') {
      // Look for a character class like [:alpha:]
      bool isClass = false;
      if (idx + 3 < glob.size() && glob[idx + 1] == ':') {
        auto classStart = idx + 2;
        for (auto end = classStart; end + 1 < glob.size(); ++end) {
          if (glob[end] == ':' && glob[end + 1] == ']') {
            StringPiece charClass(glob.data() + classStart, glob.data() + end);
            if (!addCharClass(charClass, pattern)) {
              return folly::makeUnexpected<string>(
                  "unknown character class \"" + charClass.str() + "\"");
            }
            idx = end + 1;
            isClass = true;
            break;
          }
        }
      }
      // This wasn't a character class.
      // Just treat this just as a literal '[' character.
      if (!isClass) {
        addPrevChar();
        prevChar = c;
      }
    } else {
      addPrevChar();
      prevChar = c;
    }
  }

  addPrevChar();
  pattern->push_back(GLOB_CHAR_CLASS_END);
  return idx;
}

bool GlobMatcher::addCharClass(
    StringPiece charClass,
    vector<uint8_t>* pattern) {
  auto addRange = [&](uint8_t low, uint8_t high) {
    DCHECK_LE(low, high);
    pattern->push_back(GLOB_CHAR_CLASS_RANGE);
    pattern->push_back(low);
    pattern->push_back(high);
  };

  // Character class definitions.
  // These match the POSIX Standard Locale as defined in ISO/IEC 9945-2:1993
  if (charClass == "alnum") {
    addRange('a', 'z');
    addRange('A', 'Z');
    addRange('0', '9');
    return true;
  } else if (charClass == "alpha") {
    addRange('a', 'z');
    addRange('A', 'Z');
    return true;
  } else if (charClass == "blank") {
    pattern->push_back(' ');
    pattern->push_back('\t');
    return true;
  } else if (charClass == "cntrl") {
    // POSIX locale cntrl definitions:
    // 0x00-0x1f,0x7f
    addRange(0x00, 0x1f);
    pattern->push_back(0x7f);
    return true;
  } else if (charClass == "digit") {
    addRange('0', '9');
    return true;
  } else if (charClass == "graph") {
    // POSIX locale graph definition: alnum + punct
    // This is everything from 0x21 - 0x7e
    addRange(0x21, 0x7e);
    return true;
  } else if (charClass == "lower") {
    addRange('a', 'z');
    return true;
  } else if (charClass == "print") {
    // POSIX locale print definition: alnum + punct + ' '
    // This is everything from 0x20 - 0x7e
    addRange(0x20, 0x7e);
    return true;
  } else if (charClass == "punct") {
    // POSIX locale punct definitions:
    // 0x21-0x2f, 0x3a-0x40, 0x5b-0x60, 0x7b-0x7e
    addRange(0x21, 0x2f);
    addRange(0x3a, 0x40);
    addRange(0x5b, 0x60);
    addRange(0x7b, 0x7e);
    return true;
  } else if (charClass == "space") {
    pattern->push_back(' ');
    pattern->push_back('\f');
    pattern->push_back('\n');
    pattern->push_back('\r');
    pattern->push_back('\t');
    pattern->push_back('\v');
    return true;
  } else if (charClass == "upper") {
    addRange('A', 'Z');
    return true;
  } else if (charClass == "xdigit") {
    addRange('0', '9');
    addRange('a', 'f');
    addRange('A', 'F');
    return true;
  }

  return false;
}

bool GlobMatcher::match(StringPiece text) const {
  return tryMatchAt(text, 0, 0);
}

bool GlobMatcher::tryMatchAt(
    StringPiece text,
    size_t textIdx,
    size_t patternIdx) const {
  // Loop through all opcodes in the pattern buffer.
  // It's kind of unfortunate how big and complicated this while loop is.
  //
  // It would improve readability to break this down into one function per
  // opcode, but then it would require additional conditional checks after each
  // function to see if we should break out or keep going.  Having everything
  // inlined in this single while loop makes it very easy to break out early
  // without additional checks.
  //
  // I have tried breaking this out into separate functions (and also using an
  // array lookup to find the correct opcode handler, rather than just serial
  // if checks).  Unfortunately this did result in a performance hit.
  while (patternIdx < pattern_.size()) {
    if (pattern_[patternIdx] == GLOB_LITERAL) {
      // A literal string section
      uint8_t length = pattern_[patternIdx + 1];
      const uint8_t* literal = pattern_.data() + patternIdx + 2;
      patternIdx += 2 + length;
      if (patternIdx >= pattern_.size()) {
        // This is the last section of the pattern.
        // We can exit out early if the lengths don't match.
        if (text.size() - textIdx != length) {
          return false;
        }
        return 0 == memcmp(text.data() + textIdx, literal, length);
      }
      // Not the final piece of the pattern.  We have to do the string compare
      // (unless the text remaining is too short).
      if (text.size() - textIdx < length) {
        return false;
      }
      if (0 != memcmp(text.data() + textIdx, literal, length)) {
        return false;
      }
      // Matched so far, keep going.
      textIdx += length;
    } else if (pattern_[patternIdx] == GLOB_STAR) {
      // '*' matches 0 or more characters, excluding '/'
      ++patternIdx;
      auto matchCanStartWithDot = pattern_[patternIdx] == GLOB_TRUE;
      ++patternIdx;

      // If the glob cannot match text starting with a dot, but the text
      // has a dot here, then it cannot match.
      if (!matchCanStartWithDot && textIdx < text.size() &&
          text[textIdx] == '.') {
        return false;
      }

      if (patternIdx >= pattern_.size()) {
        // This '*' is at the end of the pattern.
        // We match as long as there are no more '/' characters
        return memchr(text.data() + textIdx, '/', text.size() - textIdx) ==
            nullptr;
      } else if (pattern_[patternIdx] == GLOB_LITERAL) {
        // This '*' is followed by a string literal.
        // Jump ahead to the next place where we find this literal.  Make sure
        // we don't cross a '/'
        auto literalLength = pattern_[patternIdx + 1];
        StringPiece literalPattern{
            ByteRange(pattern_.data() + patternIdx + 2, literalLength)};
        patternIdx += 2 + literalLength;
        auto nextSlash = text.find('/', textIdx);
        while (true) {
          auto literalIdx = text.find(literalPattern, textIdx);
          if (literalIdx == StringPiece::npos) {
            // No match.
            return false;
          }
          if (nextSlash < literalIdx) {
            return false;
          }
          if (tryMatchAt(text, literalIdx + literalLength, patternIdx)) {
            return true;
          }
          // No match here.  Move forwards and try again.
          textIdx = literalIdx + 1;
        }
      } else {
        // '*' followed by another glob special, such as ? or a character
        // class.  We inefficiently try matching forwards one character at a
        // time.
        //
        // In practice this type of pattern is rare.
        while (textIdx < text.size()) {
          if (tryMatchAt(text, textIdx, patternIdx)) {
            return true;
          }
          if (text[textIdx] == '/') {
            return false;
          }
          ++textIdx;
        }
        return false;
      }
    } else if (pattern_[patternIdx] == GLOB_ENDS_WITH) {
      // Advance patternIdx to read the bool from the original GLOB_STAR.
      ++patternIdx;
      auto matchCanStartWithDot = pattern_[patternIdx] == GLOB_TRUE;

      // If the glob match is not allowed to start with a dot then we also
      // reject cases where it matches the empty string followed by a dot.
      // We intentionally do not allow `*.cpp` to match `.cpp`
      // This matches the behavior of the POSIX fnmatch() function.
      // Because any match of '*' will start from the current textIdx, we
      // can return right away if we know any match would start with an
      // illegal dot.
      if (!matchCanStartWithDot && textIdx < text.size() &&
          text[textIdx] == '.') {
        return false;
      }

      // An "ends-with" section
      uint8_t length = pattern_[patternIdx + 1];
      const uint8_t* literal = pattern_.data() + patternIdx + 2;
      if (text.size() - textIdx < length) {
        return false;
      }
      if (0 != memcmp(text.end() - length, literal, length)) {
        return false;
      }
      // The end of the text matched the desired literal.
      // Now we just have to verify that there were no '/' characters in the
      // preceding portion (that matches "*").
      return memchr(
                 text.data() + textIdx,
                 '/',
                 text.size() - (textIdx + length)) == nullptr;
    } else if (pattern_[patternIdx] == GLOB_STAR_STAR_END) {
      // This is '**' at the end of a pattern.  It matches everything else in
      // the text. However, if this matcher was created with
      // GlobOptions::IGNORE_DOTFILES, then we must ensure that none of the path
      // components in the remaining text start with a '.'.
      ++patternIdx;
      auto pathComponentInMatchCanStartWithDot =
          pattern_[patternIdx] == GLOB_TRUE;
      if (pathComponentInMatchCanStartWithDot) {
        return true;
      }

      // By construction, we know that GLOB_STAR_STAR_END is preceded by a
      // slash, so we can start from the previous character and scan the
      // remaining text for "/." If we find one, then this is not a match.
      auto searchIndex = textIdx == 0 ? 0 : textIdx - 1;
      return text.find("/.", searchIndex) == StringPiece::npos;
    } else if (pattern_[patternIdx] == GLOB_STAR_STAR_SLASH) {
      ++patternIdx;
      auto pathComponentInMatchCannotStartWithDot =
          pattern_[patternIdx] == GLOB_FALSE;

      // This is "**/"
      // It may match nothing at all, or it may match some arbitrary number of
      // characters followed by a slash.
      ++patternIdx;
      while (true) {
        if (tryMatchAt(text, textIdx, patternIdx)) {
          return true;
        }

        auto prevTextIdx = textIdx;
        textIdx = text.find('/', prevTextIdx + 1);
        if (textIdx == StringPiece::npos) {
          // No match.
          return false;
        } else if (
            pathComponentInMatchCannotStartWithDot &&
            text[prevTextIdx] == '.') {
          // Verify the path component does not start with an illegal dot
          // before proceeding.
          return false;
        }

        ++textIdx;
      }
    } else {
      // The other glob special patterns all match exactly one character.
      // Get this character now.
      if (textIdx >= text.size()) {
        return false;
      }
      uint8_t ch = text[textIdx];
      ++textIdx;

      // Git does not allow '/' to match any of these cases.
      if (ch == '/') {
        return false;
      }

      if (pattern_[patternIdx] == GLOB_CHAR_CLASS) {
        // An inclusive character class
        if (!charClassMatch(ch, &patternIdx)) {
          return false;
        }
      } else if (pattern_[patternIdx] == GLOB_CHAR_CLASS_NEGATED) {
        // An exclusive character class
        if (charClassMatch(ch, &patternIdx)) {
          return false;
        }
      } else if (pattern_[patternIdx] == GLOB_QMARK) {
        // '?' matches any character except '/'
        // (which we already excluded above)
        ++patternIdx;
      } else {
        // Unknown opcode.  This should never happen.
        XLOG(FATAL) << "bug processing glob pattern buffer at index "
                    << patternIdx;
      }
    }
  }

  return textIdx == text.size();
}

bool GlobMatcher::charClassMatch(uint8_t ch, size_t* patternIdx) const {
  size_t idx = *patternIdx + 1;
  while (true) {
    DCHECK_LT(idx, pattern_.size());
    if (pattern_[idx] == GLOB_CHAR_CLASS_END) {
      // Reached the end of the character class with no match.
      *patternIdx = idx + 1;
      return false;
    } else if (pattern_[idx] == GLOB_CHAR_CLASS_RANGE) {
      DCHECK_LT(idx + 2, pattern_.size());
      uint8_t lowBound = pattern_[idx + 1];
      uint8_t highBound = pattern_[idx + 2];
      idx += 2;
      if (lowBound <= ch && ch <= highBound) {
        // Found a match
        break;
      }
    } else {
      if (ch == pattern_[idx]) {
        // Found a match
        ++idx;
        break;
      }
      ++idx;
    }
  }

  // If we broke out of the loop then we found a match.
  // Advance patternIdx to the end of the character class.
  //
  // We just keep scanning through the data until we find GLOB_CHAR_CLASS_END.
  //
  // In theory we could put a length byte after the GLOB_CHAR_CLASS opcode,
  // similar to what we do for GLOB_LITERAL, so we could avoid scanning here.
  // However this would introduce some complications: we would potentially have
  // to re-arrange the data so it fits in 255 bytes.  (Any character class can
  // be represented in 255 bytes, but our naive literal encoding currently
  // might end up using more than 255 bytes.)  In practice character class data
  // is normally very short, so the cost of a scan doesn't really matter here.
  while (true) {
    if (pattern_[idx] == GLOB_CHAR_CLASS_END) {
      *patternIdx = idx + 1;
      return true;
    } else if (pattern_[idx] == GLOB_CHAR_CLASS_RANGE) {
      idx += 3;
    } else {
      ++idx;
    }
  }
}
} // namespace eden
} // namespace facebook
