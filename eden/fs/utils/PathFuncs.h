/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/utils/Memory.h"

#include <boost/operators.hpp>
#include <glog/logging.h>

#include <fmt/format.h>
#include <folly/Expected.h>
#include <folly/FBString.h>
#include <folly/FBVector.h>
#include <folly/Format.h>
#include <folly/String.h>
#include <folly/hash/Hash.h>
#include <optional>
#include <type_traits>

namespace facebook {
namespace eden {

/** Given a path like "foo/bar/baz" returns "baz" */
folly::StringPiece basename(folly::StringPiece path);

/** Given a path like "foo/bar/baz" returns "foo/bar" */
folly::StringPiece dirname(folly::StringPiece path);

/** Path directory separator.
 *
 * We always use a forward slash.  On Windows systems we will normalize
 * path names to alway use forward slash separators instead of backslashes.
 *
 * (This is defined as an enum value since we just want a symbolic constant,
 * and don't want an actual symbol emitted by the compiler.)
 */

enum : char { kDirSeparator = '/' };
constexpr folly::StringPiece kDirSeparatorStr{"/"};

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
 * AbsolutePath, AbsolutePathPiece: represent an absolute
 * path.  An absolute path must begin with a '/' character, and may be
 * composed with PathComponents and RelativePaths, but not with other
 * AbsolutePaths.
 *
 * Values of each of these types are immutable.
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

// Intentionally use folly::fbstring because Dir entries are keyed on
// PathComponent and the fact that folly::fbstring is 24 bytes and std::string
// is 32 bytes adds up.
using PathComponent = detail::PathComponentBase<folly::fbstring>;
using PathComponentPiece = detail::PathComponentBase<folly::StringPiece>;

using RelativePath = detail::RelativePathBase<std::string>;
using RelativePathPiece = detail::RelativePathBase<folly::StringPiece>;

using AbsolutePath = detail::AbsolutePathBase<std::string>;
using AbsolutePathPiece = detail::AbsolutePathBase<folly::StringPiece>;

namespace detail {

// Helper for equality testing, borrowed from
// folly::detail::ComparableAsStringPiece in folly/Range.h
template <typename A, typename B, typename Stored, typename Piece>
struct StoredOrPieceComparableAsStringPiece {
  enum {
    value =
        (std::is_convertible<A, folly::StringPiece>::value &&
         (std::is_same<B, Stored>::value || std::is_same<B, Piece>::value)) ||
        (std::is_convertible<B, folly::StringPiece>::value &&
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
    typename Piece // eg: Foo<StringPiece>
    >
struct PathOperators {
  // Less-than
  friend bool operator<(const Stored& a, const Stored& b) {
    return a.stringPiece() < b.stringPiece();
  }
  friend bool operator<(const Piece& a, const Stored& b) {
    return a.stringPiece() < b.stringPiece();
  }

  friend bool operator<(const Piece& a, const Piece& b) {
    return a.stringPiece() < b.stringPiece();
  }
  friend bool operator<(const Stored& a, const Piece& b) {
    return a.stringPiece() < b.stringPiece();
  }

  // Equality
  friend bool operator==(const Stored& a, const Stored& b) {
    return a.stringPiece() == b.stringPiece();
  }
  friend bool operator==(const Piece& a, const Stored& b) {
    return a.stringPiece() == b.stringPiece();
  }

  friend bool operator==(const Piece& a, const Piece& b) {
    return a.stringPiece() == b.stringPiece();
  }
  friend bool operator==(const Stored& a, const Piece& b) {
    return a.stringPiece() == b.stringPiece();
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
    return folly::StringPiece(a) == folly::StringPiece(rhs);
  }

  template <typename A, typename B>
  friend typename std::enable_if<
      StoredOrPieceComparableAsStringPiece<A, B, Stored, Piece>::value,
      bool>::type
  operator!=(const A& a, const B& rhs) {
    return folly::StringPiece(a) != folly::StringPiece(rhs);
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
        boost::totally_ordered<Piece>>::type,
    // equality operators, as boost's helpers get confused
    public PathOperators<Stored, Piece> {
 protected:
  Storage path_;

 public:
  // These type aliases are useful in other templates to be able
  // to determine the Piece and Stored counterparts for a parameter.
  using piece_type = Piece;
  using stored_type = Stored;

  /** Default construct an empty value. */
  PathBase() {}

  /** Construct from an untyped string value.
   * Applies sanity checks. */
  explicit PathBase(folly::StringPiece src) : path_(src.data(), src.size()) {
    SanityChecker()(src);
  }

  /** Construct from an untyped string value.
   * Skips sanity checks. */
  explicit PathBase(folly::StringPiece src, SkipPathSanityCheck)
      : path_(src.data(), src.size()) {}

  /** Construct from a stored variation of this type.
   * Skips sanity checks. */
  explicit PathBase(const Stored& other)
      : path_(other.stringPiece().data(), other.stringPiece().size()) {}

  /** Construct from a non-stored variation of this type.
   * Skips sanity checks. */
  explicit PathBase(const Piece& other)
      : path_(other.stringPiece().data(), other.stringPiece().size()) {}

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
  explicit PathBase(Stored&& other) : path_(std::move(other.path_)) {}

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
  explicit PathBase(std::string&& str) : path_(std::move(str)) {
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
  explicit PathBase(std::string&& str, SkipPathSanityCheck)
      : path_(std::move(str)) {
    SanityChecker()(path_);
  }

  /// Return the path as a StringPiece
  folly::StringPiece stringPiece() const {
    return folly::StringPiece{path_};
  }

  /// Return a stored copy of this path
  Stored copy() const {
    return Stored(stringPiece(), SkipPathSanityCheck());
  }

  /// Return a non-stored reference to this path
  Piece piece() const {
    return Piece(stringPiece(), SkipPathSanityCheck());
  }

  /// Implicit conversion to Piece
  /* implicit */ operator Piece() const {
    return piece();
  }

  explicit operator folly::StringPiece() const {
    return stringPiece();
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
};

/// Asserts that val is a well formed path component
struct PathComponentSanityCheck {
  void operator()(folly::StringPiece val) const {
    if (val.find(kDirSeparator) != std::string::npos) {
      throw std::domain_error(folly::to<std::string>(
          "attempt to construct a PathComponent from a string containing a "
          "directory separator: ",
          val));
    }

    if (val.empty()) {
      throw std::domain_error("cannot have an empty PathComponent");
    }

    if (val == "." || val == "..") {
      throw std::domain_error("PathComponent must not be . or ..");
    }
  }
};

/** Represents a name within a directory.
 * It is illegal for a PathComponent to contain a directory
 * separator character. */
template <typename Storage>
class PathComponentBase : public PathBase<
                              Storage,
                              PathComponentSanityCheck,
                              PathComponent,
                              PathComponentPiece> {
 public:
  // Inherit constructors
  using base_type = PathBase<
      Storage,
      PathComponentSanityCheck,
      PathComponent,
      PathComponentPiece>;
  using base_type::base_type;

  /// Forbid empty PathComponents
  PathComponentBase() = delete;
};

/**
 * An iterator over prefixes of a composed path
 *
 * Iterating yields a series of composed path elements.
 * For example, iterating the path "foo/bar/baz" will yield
 * this series of Piece elements:
 *
 * 1. "/" but only for AbsolutePath
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
class ComposedPathIterator
    : public std::iterator<std::input_iterator_tag, const Piece> {
 public:
  using position_type = folly::StringPiece::const_iterator;

  explicit ComposedPathIterator() : path_(), pos_(nullptr) {}

  ComposedPathIterator(const ComposedPathIterator& other) = default;
  ComposedPathIterator& operator=(const ComposedPathIterator& other) = default;

  /// Initialize the iterator and point to the start of the path.
  explicit ComposedPathIterator(Piece path)
      : path_(path.stringPiece()), pos_(pathBegin()) {}

  /** Initialize the iterator at an arbitrary position. */
  ComposedPathIterator(Piece path, position_type pos)
      : path_(path.stringPiece()), pos_(pos) {}

  bool operator==(const ComposedPathIterator& other) const {
    DCHECK_EQ(path_, other.path_);
    return pos_ == other.pos_;
  }

  bool operator!=(const ComposedPathIterator& other) const {
    DCHECK_EQ(path_, other.path_);
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
    CHECK_NOTNULL(pos_);
    // Return everything preceding the slash to which pos_ points.
    return Piece(folly::StringPiece(path_.begin(), pos_));
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
    if (std::is_same<Piece, AbsolutePathPiece>::value) {
      // Always start iterators at the initial "/" character, so
      // that begin() yields "/" instead of the empty string.
      return path_.begin() + 1;
    } else {
      return path_.begin();
    }
  }

  // Move the iterator forwards in the path.
  void advance() {
    if (IsReverse) {
      if (pos_ == nullptr) {
        pos_ = pathBegin();
        return;
      }
      CHECK_NE(pos_, path_.end());
    } else {
      CHECK_NOTNULL(pos_);
      if (pos_ == path_.end()) {
        pos_ = nullptr;
        return;
      }
    }

    ++pos_;
    while (pos_ < path_.end() && *pos_ != kDirSeparator) {
      ++pos_;
    }
  }

  // Move the iterator backwards in the path.
  void retreat() {
    auto stopPos = pathBegin();
    if (IsReverse) {
      CHECK_NOTNULL(pos_);
      if (pos_ <= stopPos) {
        pos_ = nullptr;
        return;
      }
    } else {
      if (pos_ == nullptr) {
        pos_ = path_.end();
        return;
      }
      CHECK_NE(pos_, stopPos);
    }

    --pos_;
    while (pos_ > stopPos && *pos_ != kDirSeparator) {
      --pos_;
    }
  }

  /// the path we're iterating over.
  folly::StringPiece path_;
  /// our current position within that path.
  position_type pos_;
};

/**
 * An iterator over components in a composed path.
 */
template <bool IsReverse>
class PathComponentIterator
    : public std::iterator<std::input_iterator_tag, const PathComponentPiece> {
 public:
  using position_type = folly::StringPiece::const_iterator;
  enum EndEnum { END };

  explicit PathComponentIterator() {}

  PathComponentIterator(const PathComponentIterator& other) = default;
  PathComponentIterator& operator=(const PathComponentIterator& other) =
      default;

  // Construct a PathComponentIterator from a composed path
  template <typename ComposedPathType>
  explicit PathComponentIterator(const ComposedPathType& path)
      : path_{path.stringPiece()} {
    if (IsReverse) {
      start_ = path_.end();
      end_ = path_.end();
      // Back start_ up to just after the last kDirSeparator
      while (start_ != path_.begin() && *(start_ - 1) != kDirSeparator) {
        --start_;
      }
    } else {
      // Skip over any leading slash, to handle absolute paths
      start_ = path_.begin();
      while (start_ != path_.end() && *start_ == kDirSeparator) {
        ++start_;
      }
      // Advance end_ until the next slash or the end of the path
      end_ = start_;
      while (end_ != path_.end() && *end_ != kDirSeparator) {
        ++end_;
      }
    }
  }

  template <typename ComposedPathType>
  explicit PathComponentIterator(const ComposedPathType& path, EndEnum)
      : path_{path.stringPiece()} {
    if (IsReverse) {
      start_ = path_.begin();
      end_ = path_.begin();
    } else {
      start_ = path_.end();
      end_ = path_.end();
    }
  }

  bool operator==(const PathComponentIterator& other) const {
    DCHECK_EQ(path_, other.path_);
    // We have to check both start_ and end_ here.
    // In most cases start_ will equal other.start_ if and only if end_ equals
    // other.end_.  However, this is not always true because of end() and
    // rend().  end_ points the same place at end() and end() - 1.
    // start_ points to the same place at rend() and rend() - 1.
    return (start_ == other.start_) && (end_ == other.end_);
  }

  bool operator!=(const PathComponentIterator& other) const {
    DCHECK_EQ(path_, other.path_);
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
    return PathComponentPiece{folly::StringPiece{start_, end_},
                              SkipPathSanityCheck{}};
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
    DCHECK_NE(start_, path_.end());
    if (end_ == path_.end()) {
      start_ = end_;
      return;
    }
    ++end_;
    start_ = end_;
    while (end_ != path_.end() && *end_ != kDirSeparator) {
      ++end_;
    }
  }

  // Move the iterator backwards in the path.
  void retreat() {
    DCHECK_NE(end_, path_.begin());
    if (start_ == path_.begin()) {
      end_ = path_.begin();
      return;
    }

    --start_;
    end_ = start_;
    while (start_ != path_.begin() && *(start_ - 1) != kDirSeparator) {
      --start_;
    }
  }

  /// the path we're iterating over.
  folly::StringPiece path_;
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
class ComposedPathBase
    : public PathBase<Storage, SanityChecker, Stored, Piece> {
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
    return Piece(
        facebook::eden::dirname(this->stringPiece()), SkipPathSanityCheck());
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

/// Asserts that val is well formed relative path
struct RelativePathSanityCheck {
  void operator()(folly::StringPiece val) const {
    if (val.startsWith(kDirSeparator)) {
      throw std::domain_error(folly::to<std::string>(
          "attempt to construct a RelativePath from an absolute path string: ",
          val));
    }
    if (val.endsWith(kDirSeparator)) {
      throw std::domain_error(folly::to<std::string>(
          "RelativePath must not end with a slash: ", val));
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
      : base_type(comp.stringPiece(), SkipPathSanityCheck()) {}

  /** Allow constructing empty */
  RelativePathBase() {}

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
        reverse_iterator{p, this->stringPiece().end()},
        reverse_iterator{p, this->stringPiece().begin()});
  }

  /** Return a reverse_iterator over this path and all parent directories,
   * including the empty path at the end.
   */
  reverse_iterator_range rallPaths() const {
    auto p = this->piece();
    return reverse_iterator_range(
        reverse_iterator{p, this->stringPiece().end()},
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
    auto parentPiece = parent.stringPiece();
    if (this->path_.size() <= parentPiece.size()) {
      return allPaths().end();
    }
    if (parentPiece.empty()) {
      // Note: this returns an iterator to an empty path.
      return allPaths().begin();
    }
    if (this->path_[parentPiece.size()] != kDirSeparator) {
      return allPaths().end();
    }
    folly::StringPiece prefix{this->path_.data(), parentPiece.size()};
    if (prefix != parentPiece) {
      return allPaths().end();
    }
    return iterator(
        this->piece(), this->stringPiece().begin() + parentPiece.size());
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
    folly::fbvector<folly::StringPiece> components;
    while (begin != end) {
      components.emplace_back(PathComponentPiece{*begin}.stringPiece());
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
  void operator()(folly::StringPiece val) const {
#ifndef _WIN32
    // This won't work on Windows. The usermode Windows path can start with
    // a drive letter in front: c:\folder\file.txt
    if (!val.startsWith(kDirSeparator)) {
      throw std::domain_error(folly::to<std::string>(
          "attempt to construct an AbsolutePath from a non-absolute string: \"",
          val,
          "\""));
    }
#endif
    if (val.size() > 1 && val.endsWith(kDirSeparator)) {
      // We do allow "/" though
      throw std::domain_error(folly::to<std::string>(
          "AbsolutePath must not end with a slash: ", val));
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
  AbsolutePathBase() : base_type(kDirSeparatorStr, SkipPathSanityCheck()) {}

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
        reverse_iterator{p, this->stringPiece().end()},
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
        throw std::runtime_error(folly::to<std::string>(
            child, " should be under ", this->stringPiece()));
      }

      // Note that a RelativePath cannot contain "../" path elements.
      if (myIter.piece() != childIter.piece()) {
        throw std::runtime_error(folly::to<std::string>(
            this->stringPiece(), " does not seem to be a prefix of ", child));
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

  /** Compose an AbsolutePath with a RelativePath */
  template <typename B>
  AbsolutePath operator+(const detail::RelativePathBase<B>& b) const {
    // A RelativePath may be empty, in which case we simply return a copy
    // of the absolute path.
    if (b.stringPiece().empty()) {
      return this->copy();
    }
    if (this->stringPiece() == kDirSeparatorStr) {
      // Special case to avoid building a string like "//foo"
      return AbsolutePath(
          folly::to<std::string>(this->stringPiece(), b.stringPiece()),
          detail::SkipPathSanityCheck());
    }
    return AbsolutePath(
        folly::to<std::string>(
            this->stringPiece(), kDirSeparatorStr, b.stringPiece()),
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
class PathSuffixIterator
    : public std::iterator<std::input_iterator_tag, const RelativePathPiece> {
 public:
  explicit PathSuffixIterator() {}
  explicit PathSuffixIterator(folly::StringPiece path, size_t start = 0)
      : path_{path}, start_{start} {}

  PathSuffixIterator(const PathSuffixIterator& other) = default;
  PathSuffixIterator& operator=(const PathSuffixIterator& other) = default;

  static PathIteratorRange<PathSuffixIterator<IsReverse>> createRange(
      folly::StringPiece p) {
    if (IsReverse) {
      auto end = PathSuffixIterator{p, p.size()};
      ++end;
      return PathIteratorRange<PathSuffixIterator<IsReverse>>(
          end, PathSuffixIterator{p, folly::StringPiece::npos});
    } else {
      return PathIteratorRange<PathSuffixIterator<IsReverse>>(
          PathSuffixIterator{p}, PathSuffixIterator{p, p.size()});
    }
  }

  bool operator==(const PathSuffixIterator& other) const {
    DCHECK_EQ(path_, other.path_);
    // We have to check both start_ and end_ here.
    // In most cases start_ will equal other.start_ if and only if end_ equals
    // other.end_.  However, this is not always true because of end() and
    // rend().  end_ points the same place at end() and end() - 1.
    // start_ points to the same place at rend() and rend() - 1.
    return start_ == other.start_;
  }

  bool operator!=(const PathSuffixIterator& other) const {
    DCHECK_EQ(path_, other.path_);
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
    return RelativePathPiece{
        folly::StringPiece{path_.begin() + start_, path_.end()},
        SkipPathSanityCheck{}};
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
    DCHECK_LT(start_, path_.size());

    // npos is used to represent one before the beginning (that is,
    // path.rsuffixes().end()).  Advance from npos to 0.
    if (start_ == folly::StringPiece::npos) {
      start_ = 0;
      return;
    }

    // In all other cases, move to just past the next /
    auto next = path_.find(kDirSeparator, start_ + 1);
    if (next == folly::StringPiece::npos) {
      start_ = path_.size();
    } else {
      start_ = next + 1;
    }
  }

  // Move the iterator backwards in the path.
  void retreat() {
    DCHECK_NE(start_, folly::StringPiece::npos);
    // If we are at the start of the string, move to npos
    if (start_ == 0) {
      start_ = folly::StringPiece::npos;
      return;
    }

    // Otherwise move to just past the previous /
    auto next =
        rfind(folly::StringPiece{path_.begin(), start_ - 1}, kDirSeparator);
    if (next == folly::StringPiece::npos) {
      start_ = 0;
    } else {
      start_ = next + 1;
    }
  }

  /**
   * The path we're iterating over.
   */
  folly::StringPiece path_;
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
  return suffix_iterator::createRange(this->stringPiece());
}

template <typename Storage>
typename RelativePathBase<Storage>::reverse_suffix_iterator_range
RelativePathBase<Storage>::rsuffixes() const {
  return reverse_suffix_iterator::createRange(this->stringPiece());
}

template <typename Storage>
typename AbsolutePathBase<Storage>::suffix_iterator_range
AbsolutePathBase<Storage>::suffixes() const {
  // The PathSuffixIterator code assumes that the StringPiece it is given is
  // relative, so for absolute paths just strip off the leading directory
  // separator.
  return suffix_iterator::createRange(this->stringPiece().subpiece(1));
}

template <typename Storage>
typename AbsolutePathBase<Storage>::reverse_suffix_iterator_range
AbsolutePathBase<Storage>::rsuffixes() const {
  // The PathSuffixIterator code assumes that the StringPiece it is given is
  // relative, so for absolute paths just strip off the leading directory
  // separator.
  return reverse_suffix_iterator::createRange(this->stringPiece().subpiece(1));
}

// Allow boost to compute hash values
template <typename A>
size_t hash_value(const detail::PathComponentBase<A>& path) {
  auto s = path.stringPiece();
  return folly::hash::SpookyHashV2::Hash64(s.begin(), s.size(), 0);
}

template <typename A>
size_t hash_value(const detail::RelativePathBase<A>& path) {
  auto s = path.stringPiece();
  return folly::hash::SpookyHashV2::Hash64(s.begin(), s.size(), 0);
}

template <typename A>
size_t hash_value(const detail::AbsolutePathBase<A>& path) {
  auto s = path.stringPiece();
  return folly::hash::SpookyHashV2::Hash64(s.begin(), s.size(), 0);
}

// Streaming operators for logging and printing
template <typename A>
std::ostream& operator<<(
    std::ostream& stream,
    const detail::PathComponentBase<A>& a) {
  stream << a.stringPiece();
  return stream;
}

template <typename A>
std::ostream& operator<<(
    std::ostream& stream,
    const detail::RelativePathBase<A>& a) {
  stream << a.stringPiece();
  return stream;
}

template <typename A>
std::ostream& operator<<(
    std::ostream& stream,
    const detail::AbsolutePathBase<A>& a) {
  stream << a.stringPiece();
  return stream;
}

// toAppend allows folly::to<> to operate on paths
template <typename A, typename String>
void toAppend(const detail::PathComponentBase<A>& a, String* result) {
  toAppend(a.stringPiece(), result);
}

template <typename A, typename String>
void toAppend(const detail::RelativePathBase<A>& a, String* result) {
  toAppend(a.stringPiece(), result);
}

template <typename A, typename String>
void toAppend(const detail::AbsolutePathBase<A>& a, String* result) {
  toAppend(a.stringPiece(), result);
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
      folly::to<std::string>(
          a.stringPiece(), kDirSeparatorStr, b.stringPiece()),
      detail::SkipPathSanityCheck());
}

/** Compose a RelativePath with a RelativePath */
template <typename A, typename B>
RelativePath operator+(
    const detail::RelativePathBase<A>& a,
    const detail::RelativePathBase<B>& b) {
  // A RelativePath may be empty, in which case we simply return
  // a copy of the other path value.
  if (a.stringPiece().empty()) {
    return b.copy();
  }
  if (b.stringPiece().empty()) {
    return a.copy();
  }
  return RelativePath(
      folly::to<std::string>(
          a.stringPiece(), kDirSeparatorStr, b.stringPiece()),
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
  CHECK_NOTNULL(pos_);
  if (pos_ < path_.end()) {
    return RelativePathPiece(
        folly::StringPiece(pos_ + 1, path_.end()),
        detail::SkipPathSanityCheck());
  } else {
    return RelativePathPiece();
  }
}
} // namespace detail

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
AbsolutePath canonicalPath(folly::StringPiece path);

/**
 * Canonicalize a path string relative to absolute path base
 *
 * If the input is a relative path, the specified base path is prepended to it.
 */
AbsolutePath canonicalPath(folly::StringPiece path, AbsolutePathPiece base);

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
    folly::StringPiece path);

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
AbsolutePath realpath(folly::StringPiece path);
template <typename T>
typename std::enable_if<folly::IsSomeString<T>::value, AbsolutePath>::type
realpath(const T& path) {
  return realpath(path.c_str());
}

/**
 * Convert an arbitrary unsanitized input string to a normalized AbsolutePath.
 *
 * This is like realpath(), but uses a folly::Expected to return an
 * AbsolutePath on success or an errno value on error.
 */
folly::Expected<AbsolutePath, int> realpathExpected(const char* path);
folly::Expected<AbsolutePath, int> realpathExpected(folly::StringPiece path);
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
    folly::StringPiece path,
    std::optional<folly::StringPiece> homeDir = std::nullopt);

/**
 * Attempt to normalize a path.
 *
 * This first attempts to normalize the path using realpath().  However, if
 * that fails (for instance, if the specified path does not exist on disk or is
 * not accessible), it falls back to using canonicalPath().
 */
AbsolutePath normalizeBestEffort(const char* path);
AbsolutePath normalizeBestEffort(folly::StringPiece path);
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
 * Convenient literals for constructing path types.
 */
inline namespace path_literals {
inline PathComponentPiece operator"" _pc(const char* str, size_t len) noexcept {
  return PathComponentPiece{folly::StringPiece{str, str + len}};
}

inline RelativePathPiece operator"" _relpath(
    const char* str,
    size_t len) noexcept {
  return RelativePathPiece{folly::StringPiece{str, str + len}};
}

inline AbsolutePathPiece operator"" _abspath(
    const char* str,
    size_t len) noexcept {
  return AbsolutePathPiece{folly::StringPiece{str, str + len}};
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
} // namespace eden
} // namespace facebook

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

namespace fmt {
template <typename Storage>
struct formatter<facebook::eden::detail::PathComponentBase<Storage>>
    : formatter<folly::StringPiece> {
  using Path = facebook::eden::detail::PathComponentBase<Storage>;
  auto format(const Path& p, format_context& ctx) {
    return formatter<folly::StringPiece>::format(p.stringPiece(), ctx);
  }
};

template <typename Storage>
struct formatter<facebook::eden::detail::AbsolutePathBase<Storage>>
    : formatter<folly::StringPiece> {
  using Path = facebook::eden::detail::AbsolutePathBase<Storage>;
  auto format(const Path& p, format_context& ctx) {
    return formatter<folly::StringPiece>::format(p.stringPiece(), ctx);
  }
};
} // namespace fmt

namespace folly {
/*
 * folly::FormatValue specializations so that path types can be used with
 * folly::format()
 *
 * Unfortunately due to the way FormatValue is implemented we have to provide
 * explicit specializations for the individual subclasses (we can't just
 * partially specialize it once for PathBase).
 *
 * The object being formatted is guaranteed to live longer than the FormatValue
 * itself, so we only need to capture the StringPiece in the constructor.
 *
 * We defer all implementation to FormatValue<StringPiece>
 */
template <typename Storage>
class FormatValue<facebook::eden::detail::PathComponentBase<Storage>>
    : public FormatValue<StringPiece> {
 public:
  using Param = facebook::eden::detail::PathComponentBase<Storage>;
  explicit FormatValue(const Param& val)
      : FormatValue<StringPiece>(val.stringPiece()) {}
};
template <typename Storage>
class FormatValue<facebook::eden::detail::RelativePathBase<Storage>>
    : public FormatValue<StringPiece> {
 public:
  using Param = facebook::eden::detail::RelativePathBase<Storage>;
  explicit FormatValue(const Param& val)
      : FormatValue<StringPiece>(val.stringPiece()) {}
};
template <typename Storage>
class FormatValue<facebook::eden::detail::AbsolutePathBase<Storage>>
    : public FormatValue<StringPiece> {
 public:
  using Param = facebook::eden::detail::AbsolutePathBase<Storage>;
  explicit FormatValue(const Param& val)
      : FormatValue<StringPiece>(val.stringPiece()) {}
};
} // namespace folly
