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
#include <folly/Hash.h>
#include <folly/String.h>
#include <boost/operators.hpp>
#include <type_traits>

namespace facebook {
namespace eden {

/** Given a path like "foo/bar/baz" returns "baz" */
folly::StringPiece basename(folly::StringPiece path);

/** Given a path like "foo/bar/baz" returns "foo/bar" */
folly::StringPiece dirname(folly::StringPiece path);


/* Some helpers for working with path composition.
 * Goals:
 *
 * 1. Be fbstring and StringPiece friendly
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

using PathComponent = detail::PathComponentBase<folly::fbstring>;
using PathComponentPiece = detail::PathComponentBase<folly::StringPiece>;

using RelativePath = detail::RelativePathBase<folly::fbstring>;
using RelativePathPiece = detail::RelativePathBase<folly::StringPiece>;

using AbsolutePath = detail::AbsolutePathBase<folly::fbstring>;
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
    typename Stored, // eg: Foo<fbstring>
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

  // Inequality
  friend bool operator!=(const Stored& a, const Stored& b) {
    return a.stringPiece() != b.stringPiece();
  }
  friend bool operator!=(const Piece& a, const Stored& b) {
    return a.stringPiece() != b.stringPiece();
  }

  friend bool operator!=(const Piece& a, const Piece& b) {
    return a.stringPiece() != b.stringPiece();
  }
  friend bool operator!=(const Stored& a, const Piece& b) {
    return a.stringPiece() != b.stringPiece();
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
 *    use either fbstring or StringPiece.
 * 2. SanityChecker defines a "Deleter" style type that is used
 *    to validate the input for the constructors that apply sanity
 *    checks.
 * 3. Stored defines the ultimate type of the variation that manages
 *    its own storage. eg: PathComponentBase<fbstring>.  We need this
 *    type to define appropriate relational operators and methods.
 * 4. Piece defines the ultimate type of the variation that has no
 *    storage of its own. eg: PathComponentBase<StringPiece>.  Similar
 *    to Stored above, we need this for relational operators and methods.
 */
template <
    typename Storage, // eg: fbstring or StringPiece
    typename SanityChecker, // "Deleter" style type for checks
    typename Stored, // eg: Foo<fbstring>
    typename Piece // eg: Foo<StringPiece>
    >
class PathBase :
    // ordering operators for this type
    public boost::totally_ordered<
        PathBase<Storage, SanityChecker, Stored, Piece>>,
    // ordering operators between Stored and Piece variants
    public boost::less_than_comparable<Stored, Piece>,
    // equality operators, as boost's helpers get confused
    public PathOperators<Stored, Piece> {
 protected:
  Storage path_;

 public:
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
          std::is_same<StorageAlias, folly::fbstring>::value>::type>
  explicit PathBase(Stored&& other) : path_(std::move(other.path_)) {}

  /** Move construct from an fbstring value.
   * Applies sanity checks.
   * The template gunk only enables this constructor if we are the
   * Stored flavor of this type.
   * */
  template <
      /* need to alias Storage as StorageAlias because we can't directly use
       * the class template parameter in the is_same check below */
      typename StorageAlias = Storage,
      typename = typename std::enable_if<
          std::is_same<StorageAlias, folly::fbstring>::value>::type>
  explicit PathBase(folly::fbstring&& str) : path_(std::move(str)) {
    SanityChecker()(path_);
  }

  /** Move construct from an fbstring value.
   * Skips sanity checks.
   * The template gunk only enables this constructor if we are the
   * Stored flavor of this type.
   * */
  template <
      /* need to alias Storage as StorageAlias because we can't directly use
       * the class template parameter in the is_same check below */
      typename StorageAlias = Storage,
      typename = typename std::enable_if<
          std::is_same<StorageAlias, folly::fbstring>::value>::type>
  explicit PathBase(folly::fbstring&& str, SkipPathSanityCheck)
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
  const Storage& value() const {
    return path_;
  }
};

/// Asserts that val is a well formed path component
struct PathComponentSanityCheck {
  void operator()(folly::StringPiece val) const {
    if (val.find('/') != std::string::npos) {
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

/** You may iterate over a composed path.
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
template <typename Piece>
class ComposedPathIterator
    : public std::iterator<std::input_iterator_tag, const Piece> {
 public:
  using position_type = folly::StringPiece::const_iterator;

  explicit ComposedPathIterator() : path_(), pos_(nullptr) {}

  ComposedPathIterator(const ComposedPathIterator& other) = default;
  ComposedPathIterator& operator=(const ComposedPathIterator& other) = default;

  /// Initialize the iterator and point to the start of the path.
  explicit ComposedPathIterator(Piece path)
      : path_(path.stringPiece()), pos_(path_.begin()) {}

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
    CHECK_NOTNULL(pos_);
    if (pos_ == path_.end()) {
      pos_ = nullptr;
      return *this;
    }

    ++pos_;
    while (pos_ < path_.end() && *pos_ != '/') {
      ++pos_;
    }

    return *this;
  }

  /// tmp = iter++;
  ComposedPathIterator operator++(int) {
    ComposedPathIterator tmp(*this);
    ++(*this); // invoke the ++iter handler above.
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

  /*
   * TODO: operator->() is not implemented
   *
   * Since the operator*() returns a new Piece and not a reference,
   * operator->() can't really be implemented correctly, as it needs to return
   * a pointer to some existing object.
   */
  // Piece* operator->() const;

 protected:
  /// the path we're iterating over.
  folly::StringPiece path_;
  /// our current position within that path.
  position_type pos_;
};

/** Iterates a composed path in reverse.
 * Iterating in reverse yields the same elements, but in reverse
 * order:
 * 1. "foo/bar/baz"
 * 2. "foo/bar"
 * 3. "foo"
 */
template <typename Piece>
class RelativePathReverseIterator : public ComposedPathIterator<Piece> {
 public:
  using ComposedPathIterator<Piece>::ComposedPathIterator;

  /** ++iter;
   * Since we are a reverse iterator, incrementing makes us go backwards.
   */
  RelativePathReverseIterator& operator++() {
    CHECK_NOTNULL(this->pos_);
    CHECK_NE(this->pos_, this->path_.begin());

    while (this->pos_ != this->path_.begin()) {
      --this->pos_;
      if (*this->pos_ == '/') {
        return *this;
      }
    }

    CHECK_EQ(this->pos_, this->path_.begin()); // terminal position.
    return *this;
  }
};

/** Iterates a composed path in reverse.
 * Iterating in reverse yields the same elements, but in reverse
 * order:
 * 1. "/foo/bar/baz"
 * 2. "/foo/bar"
 * 3. "/foo"
 * 4. "/"
 */
template <typename Piece>
class AbsolutePathReverseIterator : public ComposedPathIterator<Piece> {
 public:
  using ComposedPathIterator<Piece>::ComposedPathIterator;

  /** ++iter;
   * Since we are a reverse iterator, incrementing makes us go backwards.
   */
  AbsolutePathReverseIterator& operator++() {
    CHECK_NOTNULL(this->pos_);
    CHECK_NE(this->pos_, this->path_.begin());

    --this->pos_;
    if (this->pos_ == this->path_.begin()) {
      this->pos_ = nullptr;
      return *this;
    }

    while (*this->pos_ != '/') {
      --this->pos_;
    }
    if (this->pos_ == this->path_.begin()) {
      ++this->pos_;
    }

    return *this;
  }
};

/** Represents any number of PathComponents composed together.
 * This is a base implementation that powers both RelativePath
 * and AbsolutePath so that we can share the definition of the methods below.
 * */
template <
    typename Storage, // eg: fbstring or StringPiece
    typename SanityChecker, // "Deleter" style type for checks
    typename Stored, // eg: Foo<fbstring>
    typename Piece // eg: Foo<StringPiece>
    >
class ComposedPathBase
    : public PathBase<Storage, SanityChecker, Stored, Piece> {
 public:
  // Inherit constructors
  using base_type = PathBase<Storage, SanityChecker, Stored, Piece>;
  using base_type::base_type;

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
};

/// Asserts that val is well formed relative path
struct RelativePathSanityCheck {
  void operator()(folly::StringPiece val) const {
    if (val.startsWith('/')) {
      throw std::domain_error(folly::to<std::string>(
          "attempt to construct a RelativePath from an absolute path string: ",
          val));
    }
    if (val.endsWith('/')) {
      throw std::domain_error(folly::to<std::string>(
          "RelativePath must not end with a slash: ", val));
    }
  }
};

/** Represents any number of PathComponents composed together.
 * It is illegal for a RelativePath to begin with an absolute
 * path prefix (`/` on unix, more complex on windows, but we
 * haven't implemented that yet in any case) */
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
  using iterator = ComposedPathIterator<RelativePathPiece>;
  using reverse_iterator = RelativePathReverseIterator<RelativePathPiece>;

  iterator begin() const {
    // A RelativePath iteration skips the empty initial element.
    return ++iterator(this->piece());
  }

  iterator end() const {
    return iterator(this->piece(), nullptr);
  }

  reverse_iterator rbegin() const {
    return reverse_iterator(this->piece(), this->stringPiece().end());
  }

  reverse_iterator rend() const {
    // A RelativePath reverse iteration skips the final empty element,
    // so arrange to stop when we hit the front of the string.
    return reverse_iterator(this->piece(), this->stringPiece().begin());
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
      PathComponentPiece piece(begin->piece());
      components.emplace_back(piece.stringPiece());
      ++begin;
    }
    folly::join("/", components, this->path_);
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
};

/// Asserts that val is well formed absolute path
struct AbsolutePathSanityCheck {
  void operator()(folly::StringPiece val) const {
    if (!val.startsWith('/')) {
      throw std::domain_error(folly::to<std::string>(
          "attempt to construct an AbsolutePath from a non-absolute string: ",
          val));
    }
    if (val.size() > 1 && val.endsWith('/')) {
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
  AbsolutePathBase() : base_type("/", SkipPathSanityCheck()) {}

  // For iteration
  using iterator = ComposedPathIterator<AbsolutePathPiece>;
  using reverse_iterator = AbsolutePathReverseIterator<AbsolutePathPiece>;

  iterator begin() const {
    // The +1 allows us to deal with the case where we're iterating
    // over literally "/".  Without this +1, we would emit "/" twice
    // and we don't want that.
    return iterator(this->piece(), this->stringPiece().begin() + 1);
  }

  iterator end() const {
    return iterator(this->piece(), nullptr);
  }

  reverse_iterator rbegin() const {
    return reverse_iterator(this->piece(), this->stringPiece().end());
  }

  reverse_iterator rend() const {
    return reverse_iterator(this->piece(), nullptr);
  }

  /** Compose an AbsolutePath with a RelativePath */
  template <typename B>
  AbsolutePath operator+(const detail::RelativePathBase<B>& b) const {
    // A RelativePath may be empty, in which case we simply return a copy
    // of the absolute path.
    if (b.stringPiece().empty()) {
      return this->copy();
    }
    if (this->stringPiece() == "/") {
      // Special case to avoid building a string like "//foo"
      return AbsolutePath(
          folly::to<folly::fbstring>(this->stringPiece(), b.stringPiece()),
          detail::SkipPathSanityCheck());
    }
    return AbsolutePath(
        folly::to<folly::fbstring>(this->stringPiece(), "/", b.stringPiece()),
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
          std::is_same<StorageAlias, folly::fbstring>::value>::type>
  const char* c_str() const {
    return this->path_.c_str();
  }
};

// Allow boost to compute hash values
template <typename A>
std::size_t hash_value(const detail::PathComponentBase<A>& path) {
  auto s = path.stringPiece();
  return folly::hash::SpookyHashV2::Hash64(s.begin(), s.size(), 0);
}

template <typename A>
std::size_t hash_value(const detail::RelativePathBase<A>& path) {
  auto s = path.stringPiece();
  return folly::hash::SpookyHashV2::Hash64(s.begin(), s.size(), 0);
}

template <typename A>
std::size_t hash_value(const detail::AbsolutePathBase<A>& path) {
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
}

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
      folly::to<folly::fbstring>(a.stringPiece(), "/", b.stringPiece()),
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
      folly::to<folly::fbstring>(a.stringPiece(), "/", b.stringPiece()),
      detail::SkipPathSanityCheck());
}

/** Compose a RelativePath with a PathComponent */
template <typename A, typename B>
RelativePath operator+(
    const detail::RelativePathBase<A>& a,
    const detail::PathComponentBase<B>& b) {
  return a + RelativePathPiece(b);
}

}
}
