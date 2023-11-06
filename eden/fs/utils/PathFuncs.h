/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/utils/Memory.h"

#include <boost/operators.hpp>
#include <fmt/format.h>
#include <folly/Expected.h>
#include <folly/FBString.h>
#include <folly/FBVector.h>
#include <folly/String.h>
#include <folly/hash/Hash.h>
#include <folly/logging/xlog.h>
#include <iterator>
#include <optional>
#include <type_traits>

#include "eden/common/utils/StringConv.h"
#include "eden/fs/utils/CaseSensitivity.h"
#include "eden/fs/utils/String.h"
#include "eden/fs/utils/Throw.h"
#include "eden/fs/utils/Utf8.h"

namespace facebook::eden {

/** Given a path like "foo/bar/baz" returns "baz" */
std::string_view basename(std::string_view path);

/** Given a path like "foo/bar/baz" returns "foo/bar" */
std::string_view dirname(std::string_view path);

/**
 * Path directory separator.
 *
 * We always use a forward slash. On Windows systems, AbsolutePath only use
 * backward slashes, while RelativePath can use a mix of forward and backward
 * slashes.
 *
 * (This is defined as an enum value since we just want a symbolic constant,
 * and don't want an actual symbol emitted by the compiler.)
 */

enum : char { kDirSeparator = '/', kWinDirSeparator = '\\' };
constexpr std::string_view kDirSeparatorStr{"/"};
constexpr char kAbsDirSeparator =
    folly::kIsWindows ? kWinDirSeparator : kDirSeparator;

namespace detail {

constexpr std::string_view kUNCPrefix{"\\\\?\\"};
constexpr std::string_view kRootStr =
    folly::kIsWindows ? kUNCPrefix : kDirSeparatorStr;

inline constexpr bool isDirSeparator(char c) {
  return c == kDirSeparator || (folly::kIsWindows && c == kWinDirSeparator);
}

inline constexpr bool isDirSeparator(std::string_view str) {
  return str.size() == 1 && isDirSeparator(str[0]);
}

inline bool isAbsoluteRoot(std::string_view path) {
  return path == kRootStr;
}

inline size_t findPathSeparator(std::string_view str, size_t start = 0) {
  auto index = str.find(kDirSeparator, start);
  if (folly::kIsWindows) {
    auto winIndex = str.find(kWinDirSeparator, start);
    if (winIndex != std::string_view::npos) {
      if (index == std::string_view::npos) {
        return winIndex;
      } else {
        return std::min(index, winIndex);
      }
    }
  }

  return index;
}

inline size_t rfindPathSeparator(std::string_view str) {
  auto index = str.rfind(kDirSeparator);
  if (folly::kIsWindows) {
    auto winIndex = str.rfind(kWinDirSeparator);
    if (winIndex != std::string_view::npos) {
      if (index == std::string_view::npos) {
        return winIndex;
      } else {
        return std::max(index, winIndex);
      }
    }
  }

  return index;
}

/**
 * Moving Paths can lead to subtle bugs due to SSO (see below), to reduce the
 * chance of introducing these bugs, the various move constructor/operator of
 * Paths will perform a copy in debug/sanitized builds as a way for SSO Paths
 * to have the same behavior as non-SSO Paths when moved.
 *
 * Note that even the passed in object will always be left in a moved-from
 * state. This is to avoid hiding use-after-move issues that wouldn't show up
 * in debug/sanitized builds.
 */
constexpr bool kPathsAreCopiedOnMove = folly::kIsDebug || folly::kIsSanitize;

/**
 * Make a copy when kPathsAreCopiedOnMove is set or move otherwise
 */
template <typename T>
T move_or_copy(T& t) noexcept {
  if (kPathsAreCopiedOnMove) {
    // If this throws an exception in debug builds, just crash.
    auto copied = t;
    // Make sure that t is also destroyed by moving it into a local variable.
    [[maybe_unused]] auto moved = std::move(t);
    return copied;
  } else {
    static_assert(std::is_nothrow_move_constructible_v<T>);
    return std::move(t);
  }
}

/**
 * C++20 introduces this constructor for std::string_view, but EdenFS is C++17.
 */
inline std::string_view string_view_range(const char* begin, const char* end) {
  XDCHECK_LE(begin, end);
  return std::string_view{begin, static_cast<size_t>(end - begin)};
}

} // namespace detail

/**
 * FUSE supports components up to 1024 (FUSE_NAME_MAX) by default. For
 * compatibility with other filesystems, Eden will limit to 255.
 * https://en.wikipedia.org/wiki/Comparison_of_file_systems
 */
enum : size_t { kMaxPathComponentLength = 255 };

/* Some helpers for working with path composition.
 * Goals:
 *
 * 1. Be StringPiece friendly
 * 2. Allow strong typing to help us work with the various
 *    units of a path.
 * 3. To be able to produce a composed path string without
 *    worrying about counting or looking for slashes.
 * 4. To be able to decompose a path into a directory or base.
 *
 * Non-goals:
 *
 * 1. We don't care about canonicalization or realpath(), since we don't
 *    connect most of our paths to the filesystem VFS.
 *
 * Concepts:
 *
 * We introduce 3 types, each with a stored and non-stored variation:
 *
 * PathComponent, PathComponentPiece: represent a name within
 * a directory.  It is illegal for a PathComponent(Piece)? to
 * contain a directory separator character, to be empty, or to be
 * a relative ("." or "..") component.
 *
 * RelativePath, RelativePathPiece: represent any number of
 * PathComponent(Piece)?'s composed together.  It is illegal
 * for a RelativePath to begin or be composed with an AbsolutePath(Piece)?.
 * A RelativePath may be empty.
 *
 * AbsolutePath, AbsolutePathPiece: represent an absolute path.  An absolute
 * path must begin with a '/' character on POSIX systems and "\\?\" (properly
 * escaped) on Windows, and may be composed with PathComponents and
 * RelativePaths, but not with other AbsolutePaths.
 *
 * Values of each of these types are immutable.
 *
 * Caution:
 *
 * Moving a stored path may invalidate the pieces to it due to SSO used in
 * std::string and folly::fbstring. See ../docs/Paths.md for more details about
 * this.
 */

namespace detail {

/// A type to select the constructors that skip sanity checks
struct SkipPathSanityCheck {};

template <typename STR>
class PathComponentBase;

template <typename STR>
class RelativePathBase;

template <typename STR>
class AbsolutePathBase;

} // namespace detail

/**
 * Thrown when a PathComponent cannot be sanity checked.
 */
class PathComponentValidationError : public std::domain_error {
 public:
  explicit PathComponentValidationError(const std::string& s)
      : std::domain_error(s) {}
};

/**
 * Thrown when a PathComponent contains a directory separator.
 */
class PathComponentContainsDirectorySeparator
    : public PathComponentValidationError {
 public:
  explicit PathComponentContainsDirectorySeparator(const std::string& s)
      : PathComponentValidationError(s) {}
};

/**
 * Thrown when a PathComponent isn't valid utf-8.
 */
class PathComponentNotUtf8 : public PathComponentValidationError {
 public:
  explicit PathComponentNotUtf8(const std::string& s)
      : PathComponentValidationError(s) {}
};

// Intentionally use folly::fbstring because Dir entries are keyed on
// PathComponent and the fact that folly::fbstring is 24 bytes and std::string
// is 32 bytes adds up.
using PathComponent = detail::PathComponentBase<folly::fbstring>;
using PathComponentPiece = detail::PathComponentBase<std::string_view>;

using RelativePath = detail::RelativePathBase<std::string>;
using RelativePathPiece = detail::RelativePathBase<std::string_view>;

using AbsolutePath = detail::AbsolutePathBase<std::string>;
using AbsolutePathPiece = detail::AbsolutePathBase<std::string_view>;

enum class CompareResult {
  EQUAL,
  BEFORE,
  AFTER,
};

struct AsciiLessThanCaseInsensitive {
  static char toLower(char c) {
    if (c >= 'A' && c <= 'Z') {
      c += 'a' - 'A';
    }
    return c;
  }

  bool operator()(char lhs, char rhs) const {
    lhs = toLower(lhs);
    rhs = toLower(rhs);
    return lhs < rhs;
  }
};

namespace detail {

// Helper for equality testing, borrowed from
// folly::detail::ComparableAsStringPiece in folly/Range.h
template <typename A, typename B, typename Stored, typename Piece>
struct StoredOrPieceComparableAsStringPiece {
  enum {
    value =
        (std::is_convertible<A, std::string_view>::value &&
         (std::is_same<B, Stored>::value || std::is_same<B, Piece>::value)) ||
        (std::is_convertible<B, std::string_view>::value &&
         (std::is_same<A, Stored>::value || std::is_same<A, Piece>::value))
  };
};

/** Comparison operators.
 * This is unfortunately a little repetitive.
 * We only want to define these operators for the Stored and Piece variations
 * of the type, as we don't want to allow comparing a PathComponent against a
 * RelativePath.
 * This has to be broken out as a base class to avoid a compilation error
 * about redefining the friend operators; that happens because we instantiate
 * two flavors of the same template with the same comparison ops.
 */
template <
    typename Stored, // eg: Foo<std::string>
    typename Piece, // eg: Foo<StringPiece>
    bool IsComposed>
struct PathOperators {
  // Less-than
  friend bool operator<(const Stored& a, const Stored& b) {
    return Piece{a} < Piece{b};
  }

  friend bool operator<(const Piece& a, const Stored& b) {
    return a < Piece{b};
  }

  friend bool operator<(const Stored& a, const Piece& b) {
    return Piece{a} < b;
  }

  friend bool operator<(const Piece& a, const Piece& b) {
    return isPathPieceLess(a, b, CaseSensitivity::Sensitive);
  }

  /**
   * Test if the left piece is lexicographically before the right piece. This
   * respects the passed in CaseSensitivity.
   */
  friend bool isPathPieceLess(
      const Piece& left,
      const Piece& right,
      CaseSensitivity caseSensitive) {
    if constexpr (IsComposed && folly::kIsWindows) {
      struct LessComponentComparator {
        bool operator()(
            const typename Piece::component_iterator::value_type& left,
            const typename Piece::component_iterator::value_type& right) {
          auto leftStringPiece = left.view();
          auto rightStringPiece = right.view();
          if (caseSensitive == CaseSensitivity::Sensitive) {
            return leftStringPiece < rightStringPiece;
          } else {
            return std::lexicographical_compare(
                leftStringPiece.begin(),
                leftStringPiece.end(),
                rightStringPiece.begin(),
                rightStringPiece.end(),
                AsciiLessThanCaseInsensitive{});
          }
        }

        CaseSensitivity caseSensitive;
      };

      auto leftComponents = left.components();
      auto rightComponents = right.components();
      return std::lexicographical_compare(
          leftComponents.begin(),
          leftComponents.end(),
          rightComponents.begin(),
          rightComponents.end(),
          LessComponentComparator{caseSensitive});
    } else {
      auto leftStringPiece = left.view();
      auto rightStringPiece = right.view();
      if (caseSensitive == CaseSensitivity::Sensitive) {
        return leftStringPiece < rightStringPiece;
      } else {
        return std::lexicographical_compare(
            leftStringPiece.begin(),
            leftStringPiece.end(),
            rightStringPiece.begin(),
            rightStringPiece.end(),
            AsciiLessThanCaseInsensitive{});
      }
    }
  }

  // Equality
  friend bool operator==(const Stored& a, const Stored& b) {
    return Piece{a} == Piece{b};
  }

  friend bool operator==(const Piece& a, const Stored& b) {
    return a == Piece{b};
  }

  friend bool operator==(const Stored& a, const Piece& b) {
    return Piece{a} == b;
  }

  friend bool operator==(const Piece& a, const Piece& b) {
    return isPathPieceEqual(a, b, CaseSensitivity::Sensitive);
  }

  /**
   * Test if both pieces are equal. This respects the passed in CaseSensitivity.
   */
  friend bool isPathPieceEqual(
      const Piece& left,
      const Piece& right,
      CaseSensitivity caseSensitive) {
    if constexpr (IsComposed && folly::kIsWindows) {
      struct EqualComponentComparator {
        bool operator()(
            const typename Piece::component_iterator::value_type& left,
            const typename Piece::component_iterator::value_type& right) {
          auto leftStringPiece = left.view();
          auto rightStringPiece = right.view();
          if (caseSensitive == CaseSensitivity::Sensitive) {
            return leftStringPiece == rightStringPiece;
          } else {
            return std::equal(
                leftStringPiece.begin(),
                leftStringPiece.end(),
                rightStringPiece.begin(),
                rightStringPiece.end(),
                folly::AsciiCaseInsensitive{});
          }
        }

        CaseSensitivity caseSensitive;
      };

      auto leftComponents = left.components();
      auto rightComponents = right.components();
      return std::equal(
          leftComponents.begin(),
          leftComponents.end(),
          rightComponents.begin(),
          rightComponents.end(),
          EqualComponentComparator{caseSensitive});
    } else {
      auto leftStringPiece = left.view();
      auto rightStringPiece = right.view();
      if (caseSensitive == CaseSensitivity::Sensitive) {
        return leftStringPiece == rightStringPiece;
      } else {
        return std::equal(
            leftStringPiece.begin(),
            leftStringPiece.end(),
            rightStringPiece.begin(),
            rightStringPiece.end(),
            folly::AsciiCaseInsensitive{});
      }
    }
  }

  // Equality and Inequality vs stringy looking values.
  // We allow testing against anything that is convertible to StringPiece.
  // This template gunk generates the code for testing the following
  // combinations DRY style:
  // (Stored, convertible-to-StringPiece)
  // (convertible-to-StringPiece, Stored)
  // (Piece, convertible-to-StringPiece)
  // (convertible-to-StringPiece, Piece)
  template <typename A, typename B>
  friend typename std::enable_if<
      StoredOrPieceComparableAsStringPiece<A, B, Stored, Piece>::value,
      bool>::type
  operator==(const A& a, const B& rhs) {
    return std::string_view(a) == std::string_view(rhs);
  }

  template <typename A, typename B>
  friend typename std::enable_if<
      StoredOrPieceComparableAsStringPiece<A, B, Stored, Piece>::value,
      bool>::type
  operator!=(const A& a, const B& rhs) {
    return std::string_view(a) != std::string_view(rhs);
  }

  /**
   * Compare the 2 passed in path based on the case sensitivity.
   *
   * Returns:
   *  - CompareResult::EQUAL if both are equal according to the case sensitivity
   *  - CompareResult::BEFORE if left is lexicographically before right
   *  - CompareResult::AFTER if left is lexicographically after right
   *
   */
  friend CompareResult comparePathPiece(
      const Piece& left,
      const Piece& right,
      CaseSensitivity caseSensitive) {
    if (isPathPieceEqual(left, right, caseSensitive)) {
      return CompareResult::EQUAL;
    } else if (isPathPieceLess(left, right, caseSensitive)) {
      return CompareResult::BEFORE;
    } else {
      return CompareResult::AFTER;
    }
  }
};

/** Defines the common path methods.
 *
 * PathBase is inherited by our consumer-visible types.
 * It is templated around 4 type parameters:
 *
 * 1. Storage defines the nature of the data storage.  We only
 *    use either std::string or StringPiece.
 * 2. SanityChecker defines a "Deleter" style type that is used
 *    to validate the input for the constructors that apply sanity
 *    checks.
 * 3. Stored defines the ultimate type of the variation that manages
 *    its own storage. eg: PathComponentBase<std::string>.  We need this
 *    type to define appropriate relational operators and methods.
 * 4. Piece defines the ultimate type of the variation that has no
 *    storage of its own. eg: PathComponentBase<StringPiece>.  Similar
 *    to Stored above, we need this for relational operators and methods.
 */
template <
    typename Storage, // eg: std::string or StringPiece
    typename SanityChecker, // "Deleter" style type for checks
    typename Stored, // eg: Foo<std::string>
    typename Piece // eg: Foo<StringPiece>
    >
class PathBase :
    // ordering operators for this type
    public std::conditional<
        std::is_same<Storage, Stored>::value,
        boost::totally_ordered<Stored, Piece>,
        boost::totally_ordered<Piece>>::type {
 protected:
  Storage path_;

 public:
  // These type aliases are useful in other templates to be able
  // to determine the Piece and Stored counterparts for a parameter.
  using piece_type = Piece;
  using stored_type = Stored;

  /** Default construct an empty value. */
  constexpr PathBase() = default;

  /** Construct from an untyped string value.
   * Applies sanity checks. */
  constexpr explicit PathBase(std::string_view src)
      : path_(src.data(), src.size()) {
    SanityChecker()(src);
  }

#ifdef _WIN32
  constexpr explicit PathBase(std::wstring_view src)
      : path_(wideToMultibyteString<Storage>(src)) {
    SanityChecker()(view());
  }
#endif

  /** Construct from an untyped string value.
   * Skips sanity checks. */
  constexpr explicit PathBase(
      std::string_view src,
      SkipPathSanityCheck) noexcept(noexcept(Storage{src.data(), src.size()}))
      : path_(src.data(), src.size()) {}

  /** Construct from a stored variation of this type.
   * Skips sanity checks. */
  explicit PathBase(const Stored& other)
      : path_(other.view().data(), other.view().size()) {}

  /** Construct from a non-stored variation of this type.
   * Skips sanity checks. */
  explicit PathBase(const Piece& other)
      : path_(other.view().data(), other.view().size()) {}

  /** Move construct from a Stored value.
   * Skips sanity checks.
   * The template gunk only enables this constructor if we are the
   * Stored flavor of this type.
   * */
  template <
      /* need to alias Storage as StorageAlias because we can't directly use
       * the class template parameter in the is_same check below */
      typename StorageAlias = Storage,
      typename = typename std::enable_if<
          std::is_same<StorageAlias, std::string>::value>::type>
  constexpr explicit PathBase(Stored&& other) noexcept(
      std::is_nothrow_move_constructible_v<Storage>)
      : path_{
            kPathsAreCopiedOnMove ? Storage{other.path_}
                                  : std::move(other.path_)} {}

  /** Move construct from an std::string value.
   * Applies sanity checks.
   * The template gunk only enables this constructor if we are the
   * Stored flavor of this type.
   * */
  template <
      /* need to alias Storage as StorageAlias because we can't directly use
       * the class template parameter in the is_same check below */
      typename StorageAlias = Storage,
      typename = typename std::enable_if<
          std::is_same<StorageAlias, std::string>::value>::type>
  constexpr explicit PathBase(std::string&& str)
      : path_(detail::move_or_copy(str)) {
    SanityChecker()(path_);
  }

  /** Move construct from an std::string value.
   * Skips sanity checks.
   * The template gunk only enables this constructor if we are the
   * Stored flavor of this type.
   * */
  template <
      /* need to alias Storage as StorageAlias because we can't directly use
       * the class template parameter in the is_same check below */
      typename StorageAlias = Storage,
      typename = typename std::enable_if<
          std::is_same<StorageAlias, std::string>::value>::type>
  constexpr explicit PathBase(std::string&& str, SkipPathSanityCheck)
      : path_(detail::move_or_copy(str)) {}

  /**
   * Move constructor
   *
   * This is roughly equal to the default move constructor, but with
   * extra debugging in debug/sanitized builds.
   */
  PathBase(PathBase&& other) noexcept
      : path_(detail::move_or_copy(other.path_)) {}

  /**
   * Move assignment operator
   *
   * This is roughly equal to the default move assignment operator, but with
   * extra debugging in debug/sanitized builds.
   */
  PathBase& operator=(PathBase&& other) noexcept {
    path_ = detail::move_or_copy(other.path_);
    return *this;
  }

  /**
   * Default assignment operator and copy and move constructor.
   *
   * This is needed due to the non-default move assignment operator above.
   */
  PathBase& operator=(const PathBase&) = default;
  PathBase(const PathBase&) = default;

  /**
   * Returns the path as a std::string. Returns a copy if the stored type is not
   * a std::string, and a const reference if it is.
   *
   * Primarily used to ensure null termination or convert to API boundaries like
   * Thrift.
   */
  auto asString() const -> std::conditional_t<
      std::is_same_v<Storage, std::string>,
      const std::string&,
      std::string> {
    if constexpr (std::is_same_v<Storage, std::string>) {
      return path_;
    } else {
      return std::string{view()};
    }
  }

  /// Return the path as a std::string_view
  std::string_view view() const {
    return std::string_view{path_};
  }

  /// Return a stored copy of this path
  Stored copy() const {
    return Stored(view(), SkipPathSanityCheck());
  }

  /// Return a non-stored reference to this path
  Piece piece() const {
    return Piece(view(), SkipPathSanityCheck());
  }

  /// Implicit conversion to Piece
  /* implicit */ operator Piece() const {
    return piece();
  }

  explicit operator std::string_view() const {
    return view();
  }

  /// Return a reference to the underlying stored value
  const Storage& value() const& {
    return path_;
  }
  /**
   * If we are an rvalue-reference, return a rvalue-reference to our value.
   *
   * This allows callers to extract the string we contain if they desire.
   */
  Storage&& value() && {
    return std::move(path_);
  }

#ifdef _WIN32
  std::wstring wide() const {
    auto str = multibyteToWideString(view());
    if (std::is_same_v<Piece, RelativePathPiece>) {
      // TODO(xavierd): Not sure if this replace is still necessary, for
      // relative paths, Windows should normalize them and thus not care about
      // forward vs backward slashes.
      std::replace(str.begin(), str.end(), L'/', L'\\');
    }
    return str;
  }
#endif
};

/// Asserts that val is a well formed path component
struct PathComponentSanityCheck {
  constexpr void operator()(std::string_view val) const {
    for (auto c : val) {
      if (isDirSeparator(c)) {
        throw_<PathComponentContainsDirectorySeparator>(
            "attempt to construct a PathComponent from a string containing a "
            "directory separator: ",
            val);
      }

      if (c == '\0') {
        throw_<PathComponentValidationError>(
            "attempt to construct a PathComponent from a string containing a "
            "nul byte: ",
            val);
      }
    }

    switch (val.size()) {
      case 0:
        throw PathComponentValidationError(
            "cannot have an empty PathComponent");
      case 1:
        if ('.' == val[0]) {
          throw PathComponentValidationError("PathComponent must not be .");
        }
        break;
      case 2:
        if ('.' == val[0] && '.' == val[1]) {
          throw PathComponentValidationError("PathComponent must not be ..");
        }
        break;
    }

    if (!isValidUtf8(val)) {
      throw_<PathComponentNotUtf8>(
          "attempt to construct a PathComponent from non valid UTF8 data: ",
          val);
    }
  }
};

/** Represents a name within a directory.
 * It is illegal for a PathComponent to contain a directory
 * separator character. */
template <typename Storage>
class PathComponentBase
    : public PathBase<
          Storage,
          PathComponentSanityCheck,
          PathComponent,
          PathComponentPiece>,
      public PathOperators<PathComponent, PathComponentPiece, false> {
 public:
  // Inherit constructors
  using base_type = PathBase<
      Storage,
      PathComponentSanityCheck,
      PathComponent,
      PathComponentPiece>;
  using base_type::base_type;

  /// Forbid empty PathComponents
  explicit PathComponentBase() = delete;
};

/**
 * An iterator over prefixes of a composed path
 *
 * Iterating yields a series of composed path elements.
 * For example, iterating the path "foo/bar/baz" will yield
 * this series of Piece elements:
 *
 * 1. kRootStr but only for AbsolutePath
 * 2. "foo"
 * 3. "foo/bar"
 * 4. "foo/bar/baz"
 *
 * You may use the dirname() and basename() methods to focus
 * on the portions of interest.
 *
 * Note: ComposedPathIterator doesn't really meet all of the requirements of an
 * InputIterator (operator*() returns a value rather than a reference, and
 * operator->() isn't implemented).  Nonetheless we are still using
 * std::input_iterator_tag here, since there isn't a standard tag for iterators
 * that meet the generic Iterator requirements (section 24.2.2 of the C++17
 * standard) but not the InputIterator requirements (section 24.2.3).  If we
 * don't use input_iterator_tag this breaks other functionality, such as being
 * able to use ComposedPathIterator as vector constructor arguments.
 */
template <typename Piece, bool IsReverse>
class ComposedPathIterator {
 public:
  using iterator_category = std::input_iterator_tag;
  using value_type = const Piece;
  using difference_type = std::ptrdiff_t;
  using pointer = value_type*;
  using reference = value_type&;

  using position_type = const char*;

  explicit ComposedPathIterator() : path_(), pos_(nullptr) {}

  ComposedPathIterator(const ComposedPathIterator& other) = default;
  ComposedPathIterator& operator=(const ComposedPathIterator& other) = default;

  /// Initialize the iterator and point to the start of the path.
  explicit ComposedPathIterator(Piece path)
      : path_(path.view()), pos_(pathBegin()) {}

  /** Initialize the iterator at an arbitrary position. */
  ComposedPathIterator(Piece path, position_type pos)
      : path_(path.view()), pos_(pos) {}

  bool operator==(const ComposedPathIterator& other) const {
    XDCHECK_EQ(path_, other.path_);
    return pos_ == other.pos_;
  }

  bool operator!=(const ComposedPathIterator& other) const {
    XDCHECK_EQ(path_, other.path_);
    return pos_ != other.pos_;
  }

  /// ++iter;
  ComposedPathIterator& operator++() {
    if (IsReverse) {
      retreat();
    } else {
      advance();
    }
    return *this;
  }

  /// iter++;
  ComposedPathIterator operator++(int) {
    ComposedPathIterator tmp(*this);
    ++(*this); // invoke the ++iter handler above.
    return tmp;
  }

  /// --iter;
  ComposedPathIterator& operator--() {
    if (IsReverse) {
      advance();
    } else {
      retreat();
    }
    return *this;
  }

  /// iter--;
  ComposedPathIterator operator--(int) {
    ComposedPathIterator tmp(*this);
    --(*this); // invoke the --iter handler above.
    return tmp;
  }

  /// Returns the piece for the current iterator position.
  Piece piece() const {
    XCHECK_NE(pos_, nullptr);
    // Return everything preceding the slash to which pos_ points.
    return Piece(string_view_range(path_.data(), pos_), SkipPathSanityCheck{});
  }

  /*
   * Note: dereferencing a ComposedPathIterator returns a new
   * ComposedPathPiece, and not a reference to an existing ComposedPathPiece.
   */
  Piece operator*() const {
    return piece();
  }

  /**
   * Returns a RelativePathPiece that corresponds to the characters in the path
   * "to the right of" the current iterator position. Note that this is true
   * whether this is a forward or reverse iterator.
   */
  RelativePathPiece remainder() const;

  /*
   * TODO: operator->() is not implemented
   *
   * Since the operator*() returns a new Piece and not a reference,
   * operator->() can't really be implemented correctly, as it needs to return
   * a pointer to some existing object.
   */
  // Piece* operator->() const;

 protected:
  const char* pathBegin() {
    if (std::is_same_v<Piece, AbsolutePathPiece>) {
      // Always start iterators at the initial "/" character, so
      // that begin() yields "/" instead of the empty string.
      XDCHECK_GE(path_.size(), kRootStr.size());
      return path_.data() + kRootStr.size();
    } else {
      return path_.data();
    }
  }

  // Move the iterator forwards in the path.
  void advance() {
    if (IsReverse) {
      if (pos_ == nullptr) {
        pos_ = pathBegin();
        return;
      }
      XCHECK_NE(pos_, path_.data() + path_.size());
    } else {
      XCHECK_NE(pos_, nullptr);
      if (pos_ == path_.data() + path_.size()) {
        pos_ = nullptr;
        return;
      }
    }

    ++pos_;
    while (pos_ < path_.data() + path_.size() && !isDirSeparator(*pos_)) {
      ++pos_;
    }
  }

  // Move the iterator backwards in the path.
  void retreat() {
    auto stopPos = pathBegin();
    if (IsReverse) {
      XCHECK_NE(pos_, nullptr);
      if (pos_ <= stopPos) {
        pos_ = nullptr;
        return;
      }
    } else {
      if (pos_ == nullptr) {
        pos_ = path_.data() + path_.size();
        return;
      }
      XCHECK_NE(pos_, stopPos);
    }

    --pos_;
    while (pos_ > stopPos && !isDirSeparator(*pos_)) {
      --pos_;
    }
  }

  /// the path we're iterating over.
  std::string_view path_;
  /// our current position within that path.
  position_type pos_;
};

/**
 * An iterator over components in a composed path.
 */
template <bool IsReverse>
class PathComponentIterator {
 public:
  using iterator_category = std::input_iterator_tag;
  using value_type = const PathComponentPiece;
  using difference_type = std::ptrdiff_t;
  using pointer = value_type*;
  using reference = value_type&;

  using position_type = const char*;
  enum EndEnum { END };

  explicit PathComponentIterator() {}

  PathComponentIterator(const PathComponentIterator& other) = default;
  PathComponentIterator& operator=(const PathComponentIterator& other) =
      default;

  // Construct a PathComponentIterator from a composed path
  template <typename ComposedPathType>
  explicit PathComponentIterator(const ComposedPathType& path)
      : pathBegin_{path.view().data() + (std::is_same_v<ComposedPathType, AbsolutePathPiece> ? kRootStr.size() : 0)},
        pathEnd_{path.view().data() + path.view().size()} {
    static_assert(
        std::is_same_v<ComposedPathType, AbsolutePathPiece> ||
            std::is_same_v<ComposedPathType, RelativePathPiece>,
        "PathComponentIterator should only be constructed from a non-stored path");

    if (IsReverse) {
      start_ = pathEnd_;
      end_ = pathEnd_;
      // Back start_ up to just after the last kDirSeparator
      while (start_ != pathBegin_ && !isDirSeparator(*(start_ - 1))) {
        --start_;
      }
    } else {
      // Skip over any leading slash, to handle absolute paths
      start_ = pathBegin_;

      // Advance end_ until the next slash or the end of the path
      end_ = start_;
      while (end_ != pathEnd_ && !isDirSeparator(*end_)) {
        ++end_;
      }
    }
  }

  template <typename ComposedPathType>
  explicit PathComponentIterator(const ComposedPathType& path, EndEnum)
      : pathBegin_{path.view().data() + (std::is_same_v<ComposedPathType, AbsolutePathPiece> ? kRootStr.size() : 0)},
        pathEnd_{path.view().data() + path.view().size()} {
    static_assert(
        std::is_same_v<ComposedPathType, AbsolutePathPiece> ||
            std::is_same_v<ComposedPathType, RelativePathPiece>,
        "PathComponentIterator should only be constructed from a non-stored path");

    if (IsReverse) {
      start_ = pathBegin_;
      end_ = start_;
    } else {
      start_ = pathEnd_;
      end_ = pathEnd_;
    }
  }

  bool operator==(const PathComponentIterator& other) const {
    XDCHECK_EQ(pathBegin_, other.pathBegin_);
    XDCHECK_EQ(pathEnd_, other.pathEnd_);
    // We have to check both start_ and end_ here.
    // In most cases start_ will equal other.start_ if and only if end_ equals
    // other.end_.  However, this is not always true because of end() and
    // rend().  end_ points the same place at end() and end() - 1.
    // start_ points to the same place at rend() and rend() - 1.
    return (start_ == other.start_) && (end_ == other.end_);
  }

  bool operator!=(const PathComponentIterator& other) const {
    XDCHECK_EQ(pathBegin_, other.pathBegin_);
    XDCHECK_EQ(pathEnd_, other.pathEnd_);
    return (start_ != other.start_) || (end_ != other.end_);
  }

  /// ++iter;
  PathComponentIterator& operator++() {
    if (IsReverse) {
      retreat();
    } else {
      advance();
    }
    return *this;
  }

  /// iter++;
  PathComponentIterator operator++(int) {
    PathComponentIterator tmp(*this);
    ++(*this); // invoke the ++iter handler above.
    return tmp;
  }

  /// --iter;
  PathComponentIterator& operator--() {
    if (IsReverse) {
      advance();
    } else {
      retreat();
    }
    return *this;
  }

  /// iter--;
  PathComponentIterator operator--(int) {
    PathComponentIterator tmp(*this);
    --(*this); // invoke the --iter handler above.
    return tmp;
  }

  /// Returns the piece for the current iterator position.
  PathComponentPiece piece() const {
    return PathComponentPiece{
        string_view_range(start_, end_), SkipPathSanityCheck{}};
  }

  /*
   * Note: dereferencing a PathComponentIterator returns a new
   * PathComponentPiece, and not a reference to an existing PathComponentPiece.
   */
  PathComponentPiece operator*() const {
    return piece();
  }

  /*
   * TODO: operator->() is not implemented
   *
   * Since the operator*() returns a new Piece and not a reference,
   * operator->() can't really be implemented correctly, as it needs to return
   * a pointer to some existing object.
   */
  // Piece* operator->() const;

 private:
  // Move the iterator forwards in the path.
  void advance() {
    XDCHECK_NE(start_, pathEnd_);
    if (end_ == pathEnd_) {
      start_ = end_;
      return;
    }
    ++end_;
    start_ = end_;
    while (end_ != pathEnd_ && !isDirSeparator(*end_)) {
      ++end_;
    }
  }

  // Move the iterator backwards in the path.
  void retreat() {
    XDCHECK_NE(end_, pathBegin_);
    if (start_ == pathBegin_) {
      end_ = pathBegin_;
      return;
    }

    --start_;
    end_ = start_;
    while (start_ != pathBegin_ && !isDirSeparator(*(start_ - 1))) {
      --start_;
    }
  }

  /// the path we're iterating over.
  position_type pathBegin_;
  position_type pathEnd_;
  /// our current position within that path.
  position_type start_{nullptr};
  position_type end_{nullptr};
};

template <bool IsReverse>
class PathSuffixIterator;

/** A pair of path iterators.
 * This is used to implement the paths() and allPaths() methods.
 */
template <typename Iterator>
class PathIteratorRange {
 public:
  using iterator = Iterator;

  PathIteratorRange(iterator b, iterator e)
      : begin_(std::move(b)), end_(std::move(e)) {}

  iterator begin() const {
    return begin_;
  }
  iterator end() const {
    return end_;
  }

 private:
  iterator begin_;
  iterator end_;
};

/** Represents any number of PathComponents composed together.
 * This is a base implementation that powers both RelativePath
 * and AbsolutePath so that we can share the definition of the methods below.
 * */
template <
    typename Storage, // eg: std::string or StringPiece
    typename SanityChecker, // "Deleter" style type for checks
    typename Stored, // eg: Foo<std::string>
    typename Piece // eg: Foo<StringPiece>
    >
class ComposedPathBase : public PathBase<Storage, SanityChecker, Stored, Piece>,
                         public PathOperators<Stored, Piece, true> {
 public:
  // Inherit constructors
  using base_type = PathBase<Storage, SanityChecker, Stored, Piece>;
  using base_type::base_type;

  // Component iterator types
  using component_iterator = PathComponentIterator<false>;
  using reverse_component_iterator = PathComponentIterator<true>;
  using component_iterator_range = PathIteratorRange<component_iterator>;
  using reverse_component_iterator_range =
      PathIteratorRange<reverse_component_iterator>;

  /// Return the final component of this path
  PathComponentPiece basename() const {
    return PathComponentPiece(
        facebook::eden::basename(this->path_), SkipPathSanityCheck());
  }

  /** Return the dirname.
   * That is a non-stored reference to everything except the final
   * component of the path. */
  Piece dirname() const {
    return Piece(facebook::eden::dirname(this->view()), SkipPathSanityCheck());
  }

  /** Return an iterator range that will yield all components of this path.
   *
   * For example, iterating the relative path "foo/bar/baz" will yield
   * this series of PathComponentPiece elements:
   *
   * 1. "foo"
   * 2. "bar"
   * 3. "baz"
   *
   * Iterating the absolute path "/foo/bar/baz" would also yield the same
   * sequence.
   */
  component_iterator_range components() const {
    auto p = this->piece();
    return component_iterator_range(
        component_iterator{p}, component_iterator{p, component_iterator::END});
  }

  /** Return an iterator range that will yield all components of this path in
   * reverse.
   */
  reverse_component_iterator_range rcomponents() const {
    auto p = this->piece();
    return reverse_component_iterator_range(
        reverse_component_iterator{p},
        reverse_component_iterator{p, reverse_component_iterator::END});
  }
};

/// Asserts that val is formed of multiple well formed PathComponents.
struct ComposedPathSanityCheck {
  constexpr size_t nextSeparator(
      std::string_view val,
      size_t start,
      std::optional<char> pathSeparator) const {
    const char* data = val.data();

    for (size_t i = start; i < val.size(); i++) {
      if (pathSeparator) {
        if (data[i] == *pathSeparator) {
          return i;
        }
      } else if (isDirSeparator(data[i])) {
        return i;
      }
    }

    return std::string_view::npos;
  }

  constexpr void operator()(
      std::string_view val,
      std::optional<char> pathSeparator = std::nullopt) const {
    size_t start = 0;
    while (true) {
      auto next = nextSeparator(val, start, pathSeparator);
      if (next == std::string_view::npos) {
        break;
      }

      PathComponentSanityCheck()(val.substr(start, next - start));
      start = next + 1;
    }

    // Last component
    PathComponentSanityCheck()(val.substr(start));
  }
};

/// Asserts that val is well formed relative path
struct RelativePathSanityCheck {
  constexpr void operator()(std::string_view val) const {
    if (!val.empty()) {
      const char* data = val.data();
      if (isDirSeparator(data[0])) {
        throw_<std::domain_error>(
            "attempt to construct a RelativePath from an absolute path string: ",
            val);
      }

      if (isDirSeparator(data[val.size() - 1])) {
        throw_<std::domain_error>(
            "RelativePath must not end with a slash: ", val);
      }

      ComposedPathSanityCheck()(val);
    }
  }
};

/** Represents any number of PathComponents composed together.
 * It is illegal for a RelativePath to begin with an absolute
 * path prefix (`/` on unix, more complex on windows, but we
 * haven't implemented that yet in any case)
 *
 * A RelativePath may be the empty string.
 */
template <typename Storage>
class RelativePathBase : public ComposedPathBase<
                             Storage,
                             RelativePathSanityCheck,
                             RelativePath,
                             RelativePathPiece> {
 public:
  // Inherit constructors
  using base_type = ComposedPathBase<
      Storage,
      RelativePathSanityCheck,
      RelativePath,
      RelativePathPiece>;
  using base_type::base_type;

  /** Construct from a PathComponent */
  template <typename T>
  explicit RelativePathBase(const PathComponentBase<T>& comp)
      : base_type(comp.view(), SkipPathSanityCheck()) {}

  /** Allow constructing empty */
  RelativePathBase() = default;

  // For iteration
  using iterator = ComposedPathIterator<RelativePathPiece, false>;
  using reverse_iterator = ComposedPathIterator<RelativePathPiece, true>;
  using iterator_range = PathIteratorRange<iterator>;
  using reverse_iterator_range = PathIteratorRange<reverse_iterator>;
  // Suffix iterator types
  using suffix_iterator = PathSuffixIterator<false>;
  using suffix_iterator_range = PathIteratorRange<suffix_iterator>;
  using reverse_suffix_iterator = PathSuffixIterator<true>;
  using reverse_suffix_iterator_range =
      PathIteratorRange<reverse_suffix_iterator>;

  /**
   * Return an iterator range that will yield all parent directories of this
   * path, and then the path itself.
   *
   * For example, iterating the path "foo/bar/baz" will yield
   * this series of Piece elements:
   *
   * 1. "foo"
   * 2. "foo/bar"
   * 3. "foo/bar/baz"
   *
   * See also the suffixes() and rsuffixes() methods, which provide iterators
   * over directory suffixes.
   */
  iterator_range paths() const {
    auto p = this->piece();
    return iterator_range(++iterator{p}, iterator{p, nullptr});
  }

  /**
   * Return an iterator range that will yield all parent directories of this
   * path, and then the path itself.
   *
   * This is very similar to paths(), but also includes the empty string
   * first, to represent the current directory that this path is relative to.
   *
   * For example, iterating the path "foo/bar/baz" will yield
   * this series of Piece elements:
   *
   * 1. ""
   * 2. "foo"
   * 3. "foo/bar"
   * 4. "foo/bar/baz"
   */
  iterator_range allPaths() const {
    auto p = this->piece();
    return iterator_range(iterator{p}, iterator{p, nullptr});
  }

  /**
   * Return a reverse_iterator over all parent directories and this path.
   *
   * See also the suffixes() and rsuffixes() methods, which provide iterators
   * over directory suffixes.
   */
  reverse_iterator_range rpaths() const {
    auto p = this->piece();
    return reverse_iterator_range(
        reverse_iterator{p, this->view().data() + this->view().size()},
        reverse_iterator{p, this->view().data()});
  }

  /** Return a reverse_iterator over this path and all parent directories,
   * including the empty path at the end.
   */
  reverse_iterator_range rallPaths() const {
    auto p = this->piece();
    return reverse_iterator_range(
        reverse_iterator{p, this->view().data() + this->view().size()},
        reverse_iterator{p, nullptr});
  }

  /**
   * Return an iterator range over all directory suffixes of the path.
   *
   * For example, iterating the path "foo/bar/baz" will yield
   * this series of Piece elements:
   *
   * 1. "foo/bar/baz"
   * 2. "bar/baz"
   * 3. "baz"
   *
   * See also the paths() and rpaths() methods, which provide iterators over
   * directory prefixes.
   */
  suffix_iterator_range suffixes() const;

  /**
   * Return an iterator range over all directory suffixes of the path, from
   * back to front.
   *
   * For example, iterating the path "foo/bar/baz" will yield
   * this series of Piece elements:
   *
   * 1. "baz"
   * 2. "bar/baz"
   * 3. "foo/bar/baz"
   *
   * See also the paths() and rpaths() methods, which provide iterators over
   * directory prefixes.
   */
  reverse_suffix_iterator_range rsuffixes() const;

  /** Return an iterator to the specified parent directory of this path.
   * If parent is not a parent directory of this path, returns
   * allPaths().end().
   *
   * The resulting iterator will fall within the range
   * [allPaths.begin(), allPaths.end()]
   * It can be incremented forwards or backwards until either end of this
   * range.
   */
  iterator findParent(const RelativePathPiece& parent) const {
    auto parentPiece = parent.view();
    if (this->path_.size() <= parentPiece.size()) {
      return allPaths().end();
    }
    if (parentPiece.empty()) {
      // Note: this returns an iterator to an empty path.
      return allPaths().begin();
    }
    if (!isDirSeparator(this->path_[parentPiece.size()])) {
      return allPaths().end();
    }
    std::string_view prefix{this->path_.data(), parentPiece.size()};
    if (prefix != parentPiece) {
      return allPaths().end();
    }
    return iterator(this->piece(), this->view().data() + parentPiece.size());
  }

  /** Construct from an iterable set of PathComponents.
   * This should match iterators with values from which we can construct
   * PathComponentPiece.
   * */
  template <
      typename Iterator,
      typename = typename std::enable_if<std::is_constructible<
          PathComponentPiece,
          typename std::iterator_traits<Iterator>::reference>::value>::type>
  RelativePathBase(Iterator begin, Iterator end) {
    folly::fbvector<std::string_view> components;
    while (begin != end) {
      components.emplace_back(PathComponentPiece{*begin}.view());
      ++begin;
    }
    folly::join(kDirSeparatorStr, components, this->path_);
  }

  /** Construct from a container that holds PathComponents.
   * This should match containers of values from which we can construct
   * PathComponentPiece.
   * */
  template <
      typename Container,
      typename = typename std::enable_if<std::is_constructible<
          PathComponentPiece,
          typename Container::const_reference>::value>::type>
  explicit RelativePathBase(const Container& container)
      : RelativePathBase(container.cbegin(), container.cend()) {}

  /** Construct from an initializer list of PathComponents. */
  explicit RelativePathBase(
      const std::initializer_list<PathComponentPiece>& values)
      : RelativePathBase(values.begin(), values.end()) {}

  /// Return true if this is an empty relative path
  bool empty() const {
    return this->path_.empty();
  }

  /** Return true if this path is a subdirectory of the specified path
   * Returns false if the two paths refer to the exact same directory.
   */
  bool isSubDirOf(const RelativePathPiece& other) const {
    return this->findParent(other) != allPaths().end();
  }

  /** Return true if this path is a parent directory of the specified path
   * Returns false if the two paths refer to the exact same directory.
   */
  bool isParentDirOf(const RelativePathPiece& other) const {
    return other.findParent(*this) != other.allPaths().end();
  }
}; // namespace detail

/// Asserts that val is well formed absolute path
struct AbsolutePathSanityCheck {
  void operator()(string_view val) const {
    if (!val.starts_with(detail::kRootStr)) {
      throw_<std::domain_error>(
          "attempt to construct an AbsolutePath from a non-absolute string: \"",
          val,
          "\"");
    }
    size_t offset = detail::kRootStr.size();

    if (val.size() > 1 && val.ends_with(kDirSeparator)) {
      // We do allow "/" though
      throw_<std::domain_error>(
          "AbsolutePath must not end with a slash: ", val);
    }

    if (val.size() > offset) {
      // Ensures that components are separated by / on posix systems and \ on
      // Windows.
      ComposedPathSanityCheck()(val.substr(offset), kAbsDirSeparator);
    }
  }
};

/** An AbsolutePath must begin with an absolute path character.
 *
 * It can be produced either explicitly from a string (perhaps
 * obtained via configuration), or by composing an AbsolutePath
 * with a RelativePath or PathComponent.
 */
template <typename Storage>
class AbsolutePathBase : public ComposedPathBase<
                             Storage,
                             AbsolutePathSanityCheck,
                             AbsolutePath,
                             AbsolutePathPiece> {
 public:
  // Inherit constructors
  using base_type = ComposedPathBase<
      Storage,
      AbsolutePathSanityCheck,
      AbsolutePath,
      AbsolutePathPiece>;
  using base_type::base_type;

  // Default construct to the root of the VFS
  constexpr AbsolutePathBase() noexcept(
      noexcept(base_type(kRootStr, SkipPathSanityCheck())))
      : base_type(kRootStr, SkipPathSanityCheck()) {}

  /**
   * Building an AbsolutePath from a plain string is not supported.
   *
   * Using the function canonicalPath should always be used to ensure that the
   * path has the right format.
   */
  explicit AbsolutePathBase(std::string_view) = delete;
  explicit AbsolutePathBase(std::string&&) = delete;
#ifdef _WIN32
  explicit AbsolutePathBase(std::wstring_view) = delete;
#endif

  // For iteration
  using iterator = ComposedPathIterator<AbsolutePathPiece, false>;
  using reverse_iterator = ComposedPathIterator<AbsolutePathPiece, true>;
  using iterator_range = PathIteratorRange<iterator>;
  using reverse_iterator_range = PathIteratorRange<reverse_iterator>;
  // Suffix iterator types
  using suffix_iterator = PathSuffixIterator<false>;
  using suffix_iterator_range = PathIteratorRange<suffix_iterator>;
  using reverse_suffix_iterator = PathSuffixIterator<true>;
  using reverse_suffix_iterator_range =
      PathIteratorRange<reverse_suffix_iterator>;

  iterator_range paths() const {
    auto p = this->piece();
    return iterator_range(iterator{p}, iterator{p, nullptr});
  }

  reverse_iterator_range rpaths() const {
    auto p = this->piece();
    return reverse_iterator_range(
        reverse_iterator{p, this->view().data() + this->view().size()},
        reverse_iterator{p, nullptr});
  }

  /**
   * Return an iterator range over all suffixes of the path.
   *
   * For example, iterating the path "/foo/bar/baz" will yield
   * this series of Piece elements:
   *
   * 1. "foo/bar/baz"
   * 2. "bar/baz"
   * 3. "baz"
   */
  suffix_iterator_range suffixes() const;

  /**
   * Return an iterator range over all suffixes of the path, from back to
   * front.
   *
   * For example, iterating the path "/foo/bar/baz" will yield
   * this series of Piece elements:
   *
   * 1. "baz"
   * 2. "bar/baz"
   * 3. "foo/bar/baz"
   */
  reverse_suffix_iterator_range rsuffixes() const;

  /**
   * This must be equal to or an ancestor of the specified path.
   * If `this` is "/foo" and `child` is "/foo/bar/baz", then this returns
   * `"bar/baz"_relpath`. If `this` and `child` are equal, then this
   * returns `RelativePathPiece()`.
   */
  RelativePathPiece relativize(AbsolutePathPiece child) const {
    auto myPaths = this->paths();
    auto childPaths = child.paths();
    auto myIter = myPaths.begin();
    auto childIter = childPaths.begin();
    while (true) {
      if (childIter == childPaths.end()) {
        throw_<std::runtime_error>(child, " should be under ", this->view());
      }

      // Note that a RelativePath cannot contain "../" path elements.
      if (myIter.piece() != childIter.piece()) {
        throw_<std::runtime_error>(
            this->view(), " does not seem to be a prefix of ", child);
      }

      myIter++;
      if (myIter != myPaths.end()) {
        childIter++;
      } else {
        // We do not want to increment childIter here or else we will be
        // missing a path component when we call remainder() after the while
        // loop.
        break;
      }
    }

    return childIter.remainder();
  }

  /**
   * Strip the UNC prefix and return a device-absolute path.
   *
   * Only use this to avoid leaking UNC paths out of EdenFS. In all other
   * cases, prefer the stringPiece method.
   *
   * This does nothing more than what stringPiece does on non-Windows.
   */
  std::string_view viewWithoutUNC() const {
    if (folly::kIsWindows) {
      return this->view().substr(detail::kUNCPrefix.size());
    } else {
      return this->view();
    }
  }

  std::string stringWithoutUNC() const {
    return std::string{viewWithoutUNC()};
  }

  /** Compose an AbsolutePath with a RelativePath */
  template <typename B>
  AbsolutePath operator+(const detail::RelativePathBase<B>& b) const {
    // A RelativePath may be empty, in which case we simply return a copy
    // of the absolute path.
    if (b.view().empty()) {
      return this->copy();
    }
    if (isAbsoluteRoot(this->view())) {
      // Special case to avoid building a string like "//foo"
      return AbsolutePath(
          fmt::format("{}{}", this->view(), b.view()),
          detail::SkipPathSanityCheck());
    }
    return AbsolutePath(
        fmt::format(
            "{}{}{}",
            this->view(),
            kAbsDirSeparator,
            fmt::join(b.components(), std::string_view{&kAbsDirSeparator, 1})),
        detail::SkipPathSanityCheck());
  }

  /** Compose an AbsolutePath with a PathComponent */
  template <typename B>
  AbsolutePath operator+(const detail::PathComponentBase<B>& b) const {
    return *this + RelativePathPiece(b);
  }

  /** Convert to a c-string for use in syscalls
   * The template gunk only enables this constructor if we are the
   * Stored flavor of this type.
   * */
  template <
      /* need to alias Storage as StorageAlias because we can't directly use
       * the class template parameter in the is_same check below */
      typename StorageAlias = Storage,
      typename = typename std::enable_if<
          std::is_same<StorageAlias, std::string>::value>::type>
  const char* c_str() const {
    return this->path_.c_str();
  }
};

/**
 * An iterator over suffixes in a composed path.
 *
 * PathSuffixIterator always returns RelativePathPiece objects, even when
 * iterating over an AbsolutePath.  This is intentional, since the suffixes
 * are relative to some other location inside the path.
 *
 * For example, when iterating forwards over the path "foo/bar/baz", the
 * iterator yields:
 *   "foo/bar/baz"
 *   "bar/baz"
 *   "baz"
 *
 * When iterating in reverse it yeilds:
 *   "baz"
 *   "bar/baz"
 *   "foo/bar/baz"
 */
template <bool IsReverse>
class PathSuffixIterator {
 public:
  using iterator_category = std::input_iterator_tag;
  using value_type = const RelativePathPiece;
  using difference_type = std::ptrdiff_t;
  using pointer = value_type*;
  using reference = value_type&;

  explicit PathSuffixIterator() {}
  explicit PathSuffixIterator(std::string_view path, size_t start = 0)
      : path_{path}, start_{start} {}

  PathSuffixIterator(const PathSuffixIterator& other) = default;
  PathSuffixIterator& operator=(const PathSuffixIterator& other) = default;

  static PathIteratorRange<PathSuffixIterator<IsReverse>> createRange(
      std::string_view p) {
    if (IsReverse) {
      auto end = PathSuffixIterator{p, p.size()};
      ++end;
      return PathIteratorRange<PathSuffixIterator<IsReverse>>(
          end, PathSuffixIterator{p, std::string_view::npos});
    } else {
      return PathIteratorRange<PathSuffixIterator<IsReverse>>(
          PathSuffixIterator{p}, PathSuffixIterator{p, p.size()});
    }
  }

  bool operator==(const PathSuffixIterator& other) const {
    XDCHECK_EQ(path_, other.path_);
    // We have to check both start_ and end_ here.
    // In most cases start_ will equal other.start_ if and only if end_ equals
    // other.end_.  However, this is not always true because of end() and
    // rend().  end_ points the same place at end() and end() - 1.
    // start_ points to the same place at rend() and rend() - 1.
    return start_ == other.start_;
  }

  bool operator!=(const PathSuffixIterator& other) const {
    XDCHECK_EQ(path_, other.path_);
    return start_ != other.start_;
  }

  /// ++iter;
  PathSuffixIterator& operator++() {
    if (IsReverse) {
      retreat();
    } else {
      advance();
    }
    return *this;
  }

  /// iter++;
  PathSuffixIterator operator++(int) {
    PathSuffixIterator tmp(*this);
    ++(*this); // invoke the ++iter handler above.
    return tmp;
  }

  /// --iter;
  PathSuffixIterator& operator--() {
    if (IsReverse) {
      advance();
    } else {
      retreat();
    }
    return *this;
  }

  /// iter--;
  PathSuffixIterator operator--(int) {
    PathSuffixIterator tmp(*this);
    --(*this); // invoke the --iter handler above.
    return tmp;
  }

  /// Returns the piece for the current iterator position.
  RelativePathPiece piece() const {
    return RelativePathPiece{path_.substr(start_), SkipPathSanityCheck{}};
  }

  /*
   * Note: dereferencing a PathSuffixIterator returns a new
   * RelativePathPiece, and not a reference to an existing RelativePathPiece.
   */
  RelativePathPiece operator*() const {
    return piece();
  }

  /*
   * TODO: operator->() is not implemented
   *
   * Since the operator*() returns a new Piece and not a reference,
   * operator->() can't really be implemented correctly, as it needs to return
   * a pointer to some existing object.
   */
  // Piece* operator->() const;

 private:
  // Move the iterator forwards in the path.
  void advance() {
    XDCHECK_LT(start_, path_.size());

    // npos is used to represent one before the beginning (that is,
    // path.rsuffixes().end()).  Advance from npos to 0.
    if (start_ == std::string_view::npos) {
      start_ = 0;
      return;
    }

    // In all other cases, move to just past the next /
    auto next = findPathSeparator(path_, start_ + 1);

    if (next == std::string_view::npos) {
      start_ = path_.size();
    } else {
      start_ = next + 1;
    }
  }

  // Move the iterator backwards in the path.
  void retreat() {
    XDCHECK_NE(start_, std::string_view::npos);
    // If we are at the start of the string, move to npos
    if (start_ == 0) {
      start_ = std::string_view::npos;
      return;
    }

    // Otherwise move to just past the previous /
    auto next = rfindPathSeparator(std::string_view{path_.data(), start_ - 1});

    if (next == std::string_view::npos) {
      start_ = 0;
    } else {
      start_ = next + 1;
    }
  }

  /**
   * The path we're iterating over.
   */
  std::string_view path_;
  /**
   * Our current position within that path.
   *
   * This is the start of the current subpiece that we represent.  The end of
   * the subpiece is always path_.end().
   *
   * For reverse iteration, we use StringPiece::npos to represent the end.
   * That is, when retreat() is called when (start_ == 0), we move to
   * StringPiece::npos next.
   */
  size_t start_{0};
};

template <typename Storage>
typename RelativePathBase<Storage>::suffix_iterator_range
RelativePathBase<Storage>::suffixes() const {
  return suffix_iterator::createRange(this->view());
}

template <typename Storage>
typename RelativePathBase<Storage>::reverse_suffix_iterator_range
RelativePathBase<Storage>::rsuffixes() const {
  return reverse_suffix_iterator::createRange(this->view());
}

template <typename Storage>
typename AbsolutePathBase<Storage>::suffix_iterator_range
AbsolutePathBase<Storage>::suffixes() const {
  // The PathSuffixIterator code assumes that the StringPiece it is given is
  // relative, so for absolute paths just strip off the leading directory
  // separator.
  return suffix_iterator::createRange(this->view().substr(kRootStr.size()));
}

template <typename Storage>
typename AbsolutePathBase<Storage>::reverse_suffix_iterator_range
AbsolutePathBase<Storage>::rsuffixes() const {
  // The PathSuffixIterator code assumes that the StringPiece it is given is
  // relative, so for absolute paths just strip off the leading directory
  // separator.
  return reverse_suffix_iterator::createRange(
      this->view().substr(kRootStr.size()));
}

// Allow boost to compute hash values
template <typename A>
size_t hash_value(const detail::PathComponentBase<A>& path) {
  auto s = path.view();
  return folly::hash::SpookyHashV2::Hash64(s.data(), s.size(), 0);
}

template <
    typename Storage,
    typename SanityChecker,
    typename Stored,
    typename Piece>
size_t hash_value(
    const detail::ComposedPathBase<Storage, SanityChecker, Stored, Piece>&
        path) {
  if (folly::kIsWindows) {
    folly::hash::SpookyHashV2 hash{};

    for (const auto component : path.components()) {
      auto s = component.view();
      hash.Update(s.data(), s.size());
    }

    uint64_t hash1, hash2;
    hash.Final(&hash1, &hash2);

    return hash1;
  } else {
    auto s = path.view();
    return folly::hash::SpookyHashV2::Hash64(s.data(), s.size(), 0);
  }
}

// Streaming operators for logging and printing
template <typename A>
std::ostream& operator<<(
    std::ostream& stream,
    const detail::PathComponentBase<A>& a) {
  stream << a.view();
  return stream;
}

template <typename A>
std::ostream& operator<<(
    std::ostream& stream,
    const detail::RelativePathBase<A>& a) {
  stream << a.view();
  return stream;
}

template <typename A>
std::ostream& operator<<(
    std::ostream& stream,
    const detail::AbsolutePathBase<A>& a) {
  stream << a.view();
  return stream;
}

} // namespace detail

// I'm not really a fan of operator overloading, but these
// are reasonably clear in intent; the `+` operator can be used
// to compose certain of the path types together in certain,
// well-defined orders.  Composition always yields the Stored flavor
// of the resultant type.

/** Compose two PathComponents to yield a RelativePath */
template <typename A, typename B>
RelativePath operator+(
    const detail::PathComponentBase<A>& a,
    const detail::PathComponentBase<B>& b) {
  // PathComponents can never be empty, so this is always a simple
  // join around a "/" character.
  return RelativePath(
      fmt::format("{}{}{}", a.view(), kDirSeparatorStr, b.view()),
      detail::SkipPathSanityCheck());
}

/** Compose a RelativePath with a RelativePath */
template <typename A, typename B>
RelativePath operator+(
    const detail::RelativePathBase<A>& a,
    const detail::RelativePathBase<B>& b) {
  // A RelativePath may be empty, in which case we simply return
  // a copy of the other path value.
  if (a.view().empty()) {
    return b.copy();
  }
  if (b.view().empty()) {
    return a.copy();
  }
  return RelativePath(
      fmt::format("{}{}{}", a.view(), kDirSeparatorStr, b.view()),
      detail::SkipPathSanityCheck());
}

/** Compose a RelativePath with a PathComponent */
template <typename A, typename B>
RelativePath operator+(
    const detail::RelativePathBase<A>& a,
    const detail::PathComponentBase<B>& b) {
  return a + RelativePathPiece(b);
}

namespace detail {
template <typename Piece, bool IsReverse>
RelativePathPiece ComposedPathIterator<Piece, IsReverse>::remainder() const {
  XCHECK_NE(pos_, nullptr);
  if (pos_ < path_.data() + path_.size()) {
    return RelativePathPiece(
        string_view_range(pos_ + 1, path_.data() + path_.size()),
        detail::SkipPathSanityCheck());
  } else {
    return RelativePathPiece();
  }
}
} // namespace detail

constexpr AbsolutePathPiece kRootAbsPath = AbsolutePathPiece{};

/**
 * Get the current working directory, as an AbsolutePath.
 */
AbsolutePath getcwd();

/**
 * Canonicalize a path string.
 *
 * This removes duplicate "/" characters, and resolves "/./" and "/../"
 * components.
 *
 * Note that we intentially convert a leading "//" to "/
 * (e.g., "//foo" --> "/foo").  (POSIX specifies that a leading "//" has
 * special platform-defined behavior, so other libraries sometimes leave it
 * as-is instead of replacing it with just one "/".)
 *
 * This is purely a string processing function.  The path in question does not
 * need to exist.  If the path refers to symbolic links, these are not
 * resolved.
 *
 * If the path is relative, the current working directory is prepended to it.
 */
AbsolutePath canonicalPath(std::string_view path);

/**
 * Canonicalize a path string relative to absolute path base
 *
 * If the input is a relative path, the specified base path is prepended to it.
 */
AbsolutePath canonicalPath(std::string_view path, AbsolutePathPiece base);

/**
 * Canonicalize a path string relative to a relative path base
 *
 * Returns a RelativePath that does not start with ".." or fails and returns:
 *  error code EPERM if path is an absolute path
 *  error code EXDEV if the return value would start with "../"
 *                   e.g. base="a" and path="../.."
 */
folly::Expected<RelativePath, int> joinAndNormalize(
    RelativePathPiece base,
    string_view path);

/**
 * Convert an arbitrary unsanitized input string to a normalized AbsolutePath.
 *
 * This resolves symlinks, as well as "." and ".." components in the input
 * path.  If the input path is a relative path it is converted into an absolute
 * one.
 *
 * This will throw an exception if the specified path does not exist, or if it
 * or one of its parent directories is inaccessible.
 *
 * You can use canonicalPath() instead if you just want to normalize the path
 * string without attempting to resolve symlinks.  canonicalPath() will succeed
 * even if the input path does not exist.
 *
 * You can use normalizeBestEffort() for a hybrid approach that attempts to
 * resolve symlinks using realpath() if possible, but falls back to
 * canonicalPath() if that fails.
 */
AbsolutePath realpath(const char* path);
AbsolutePath realpath(std::string_view path);
template <typename T>
typename std::enable_if<folly::IsSomeString<T>::value, AbsolutePath>::type
realpath(const T& path) {
  return realpath(path.c_str());
}

/** Returns the path to the currently running executable.
 * This can fail for example if the executable has been renamed or
 * deleted while it is running.
 * This path is absolute but is not guaranteed to be canonical
 * and my be prone to TOCTOU issues.
 */
AbsolutePath executablePath();

/**
 * Convert an arbitrary unsanitized input string to a normalized AbsolutePath.
 *
 * This is like realpath(), but uses a folly::Expected to return an
 * AbsolutePath on success or an errno value on error.
 */
folly::Expected<AbsolutePath, int> realpathExpected(const char* path);
folly::Expected<AbsolutePath, int> realpathExpected(std::string_view path);
template <typename T>
typename std::enable_if<
    folly::IsSomeString<T>::value,
    folly::Expected<AbsolutePath, int>>::type
realpathExpected(const T& path) {
  return realpathExpected(path.c_str());
}

/**
 * Return a new path with `~` replaced by the path to the current
 * user's home directory.  This function does not support expanding
 * the home dir of arbitrary users, and will throw an exception
 * if the string starts with `~` but not `~/`.
 *
 * The replacement for `~` is taken from the homeDir parameter
 * variable if set.  If `!homeDir.has_value()` an exception is thrown.
 *
 * On successful expansion of the tilde, the resultant path will
 * be passed through canonicalPath() and returned.
 *
 * If path doesn't begin with `~` then it will be passed through
 * canonicalPath() and returned.
 *
 * If the effective home dir value is the empty string, an
 * exception is thrown.
 */
AbsolutePath expandUser(
    string_view path,
    std::optional<std::string_view> homeDir = std::nullopt);

/**
 * Attempt to normalize a path.
 *
 * This first attempts to normalize the path using realpath().  However, if
 * that fails (for instance, if the specified path does not exist on disk or is
 * not accessible), it falls back to using canonicalPath().
 */
AbsolutePath normalizeBestEffort(const char* path);
AbsolutePath normalizeBestEffort(std::string_view path);
template <typename T>
typename std::enable_if<folly::IsSomeString<T>::value, AbsolutePath>::type
normalizeBestEffort(const T& path) {
  return normalizeBestEffort(path.c_str());
}

/**
 * Splits a path into the first component and the remainder of the path.
 * If the path has only one component, the remainder will be empty. If the
 * path is empty, an exception is thrown.
 */
std::pair<PathComponentPiece, RelativePathPiece> splitFirst(
    RelativePathPiece path);

/**
 * Throws std::system_error with ENAMETOOLONG if the given PathComponent is
 * longer than kMaxPathComponentLength.
 */
void validatePathComponentLength(PathComponentPiece name);

/**
 * Ensure that the specified path exists as a directory.
 *
 * This creates the specified directory if necessary, creating any parent
 * directories as required as well.
 *
 * Returns true if the directory was created, and false if it already existed.
 *
 * Throws an exception on error, including if the path or one of its parent
 * directories is a file rather than a directory.
 */
bool ensureDirectoryExists(AbsolutePathPiece path);

/**
 * Recursively remove a directory tree.
 *
 * Returns false if the directory did not exist in the first place, and true if
 * the directory was successfully removed.  Throws an exception on error.
 */
bool removeRecursively(AbsolutePathPiece path);

/**
 * Remove a file or directory.
 *
 * Returns false if the file/directory did not exist, and true if it was
 * successfully removed.  Throws an exception on error.
 */
bool removeFileWithAbsolutePath(AbsolutePathPiece path);

/**
 * Rename a file or directory
 *
 * It will throw an exception on error.
 */
void renameWithAbsolutePath(
    AbsolutePathPiece srcPath,
    AbsolutePathPiece destPath);

/**
 * Convert an arbitrary Thrift path to a canonicalized AbsolutePath
 *
 * May throw if the path is malformed.
 */
inline AbsolutePath absolutePathFromThrift(std::string_view path) {
  return canonicalPath(path);
}

/**
 * Convert an AbsolutePath to a Thrift path.
 *
 * In particular on Windows, AbsolutePath are UNC paths internally, but the UNC
 * prefix is stripped when sending the path to Thrift.
 */
inline std::string absolutePathToThrift(AbsolutePathPiece path) {
  return path.stringWithoutUNC();
}

/**
 * Convenient literals for constructing path types.
 */
inline namespace path_literals {
constexpr inline PathComponentPiece operator"" _pc(
    const char* str,
    size_t len) noexcept {
  return PathComponentPiece{std::string_view{str, len}};
}

inline RelativePathPiece operator"" _relpath(
    const char* str,
    size_t len) noexcept {
  return RelativePathPiece{std::string_view{str, len}};
}
} // namespace path_literals

/**
 * Gets memory usage of the path inside the RelativePathBase
 */
template <typename StringType>
size_t estimateIndirectMemoryUsage(
    const detail::RelativePathBase<StringType>& path) {
  return estimateIndirectMemoryUsage(path.value());
}
} // namespace facebook::eden

namespace std {
/* Allow std::hash to operate on these types */

template <typename A>
struct hash<facebook::eden::detail::PathComponentBase<A>> {
  size_t operator()(
      const facebook::eden::detail::PathComponentBase<A>& s) const {
    return facebook::eden::detail::hash_value(s);
  }
};

template <typename A>
struct hash<facebook::eden::detail::RelativePathBase<A>> {
  size_t operator()(
      const facebook::eden::detail::RelativePathBase<A>& s) const {
    return facebook::eden::detail::hash_value(s);
  }
};

template <typename A>
struct hash<facebook::eden::detail::AbsolutePathBase<A>> {
  size_t operator()(
      const facebook::eden::detail::AbsolutePathBase<A>& s) const {
    return facebook::eden::detail::hash_value(s);
  }
};
} // namespace std

template <typename Storage>
struct fmt::formatter<facebook::eden::detail::PathComponentBase<Storage>>
    : formatter<string_view> {
  using Path = facebook::eden::detail::PathComponentBase<Storage>;

  template <typename Context>
  auto format(const Path& p, Context& ctx) const {
    return formatter<string_view>::format(p.view(), ctx);
  }
};

template <typename Storage>
struct fmt::formatter<facebook::eden::detail::AbsolutePathBase<Storage>>
    : formatter<string_view> {
  using Path = facebook::eden::detail::AbsolutePathBase<Storage>;

  template <typename Context>
  auto format(const Path& p, Context& ctx) const {
    return formatter<string_view>::format(p.view(), ctx);
  }
};

template <typename Storage>
struct fmt::formatter<facebook::eden::detail::RelativePathBase<Storage>>
    : formatter<string_view> {
  using Path = facebook::eden::detail::RelativePathBase<Storage>;

  template <typename Context>
  auto format(const Path& p, Context& ctx) const {
    return formatter<string_view>::format(p.view(), ctx);
  }
};
