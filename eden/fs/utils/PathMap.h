/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include <folly/FBVector.h>
#include <folly/Portability.h>
#include <algorithm>
#include <functional>
#include <iterator>
#include <utility>
#include "eden/fs/utils/CaseSensitivity.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/utils/Throw.h"

namespace facebook::eden {

/** An associative container that maps from one of our path types to an
 * arbitrary value type.
 *
 * This is similar to std::map but has a couple of different properties:
 * - lookups can be made using the Piece (non-stored) variant of the key
 *   type and won't require allocation just for the lookup.
 * - The storage is a vector maintained in sorted order using a binary
 *   search (std::lower_bound). Out-of-order inserts require moving
 *   the guts of the vector around to make space and are therefore slower
 *   than the equivalent std::map.  If bulk insert performance is critical,
 *   it is better to pre-sort the data to be inserted.
 * - Since insert and erase operations move the vector contents around,
 *   those operations invalidate iterators.
 */
template <typename Value, typename Key = PathComponent>
class PathMap : private folly::fbvector<std::pair<Key, Value>> {
  using Pair = std::pair<Key, Value>;
  using Vector = folly::fbvector<Pair>;
  using Piece = typename Key::piece_type;
  using Allocator = typename Vector::allocator_type;

  // Comparator that knows how compare Stored and Piece in the vector.
  struct Compare {
    explicit Compare(CaseSensitivity caseSensitive)
        : caseSensitive_{caseSensitive} {}

    // Compare two values that are convertible to the Piece type.
    template <typename A, typename B>
    typename std::enable_if<
        std::is_convertible<A, Piece>::value &&
            std::is_convertible<B, Piece>::value,
        bool>::type
    operator()(const A& a, const B& b) const {
      return isPathPieceLess(Piece(a), Piece(b), caseSensitive_);
    }

    // Compare a Piece-convertible value against the stored Pair.
    template <typename A, typename B, typename C>
    typename std::enable_if<
        std::is_convertible<A, Piece>::value &&
            std::is_convertible<B, Piece>::value,
        bool>::type
    operator()(const A& a, const std::pair<B, C>& rhs) const {
      return isPathPieceLess(Piece(a), Piece(rhs.first), caseSensitive_);
    }

    // Compare the stored Pair against a Piece-convertible value.
    template <typename A, typename B, typename C>
    typename std::enable_if<
        std::is_convertible<A, Piece>::value &&
            std::is_convertible<B, Piece>::value,
        bool>::type
    operator()(const std::pair<A, C>& lhs, const B& b) const {
      return isPathPieceLess(Piece(lhs.first), Piece(b), caseSensitive_);
    }

    CaseSensitivity caseSensitive_{kPathMapDefaultCaseSensitive};
  };

  // Hold an instance of the comparator.
  Compare compare_;

 public:
  // Various type aliases to satisfy container concepts.
  using key_type = Key;
  using mapped_type = Value;
  using value_type = typename Vector::value_type;
  using key_compare = Compare;
  using allocator_type = Allocator;
  using reference = Pair&;
  using const_reference = const Pair&;
  using iterator = typename Vector::iterator;
  using const_iterator = typename Vector::const_iterator;
  using size_type = typename Vector::size_type;
  using difference_type = typename Vector::difference_type;
  using pointer = Pair*;
  using const_pointer = const Pair*;
  using reverse_iterator = typename Vector::reverse_iterator;
  using const_reverse_iterator = typename Vector::const_reverse_iterator;

  // Construct empty.
  explicit PathMap(CaseSensitivity caseSensitive) : compare_(caseSensitive) {}

  // Populate from an initializer_list.
  PathMap(std::initializer_list<value_type> init, CaseSensitivity caseSensitive)
      : PathMap(init.begin(), init.end(), caseSensitive) {}

  // Populate from a pair of input iterators.
  template <typename InputIterator>
  PathMap(
      InputIterator first,
      InputIterator last,
      CaseSensitivity caseSensitive)
      : compare_(caseSensitive) {
    // The std::distance call is O(1) if the iterators are random-access, but
    // O(n) otherwise.  We're fine with the O(n) on the basis that if n is large
    // enough to matter, the cost of iterating will be dwarfed by the cost
    // of growing the storage several times during population.
    this->reserve(std::distance(first, last));
    for (; first != last; ++first) {
      insert(*first);
    }
  }

  // Inherit the underlying vector copy/assignment.
  PathMap(const PathMap& other) : Vector(other), compare_(other.compare_) {}
  PathMap& operator=(const PathMap& other) {
    PathMap(other).swap(*this);
    return *this;
  }

  // inherit Move construction.
  PathMap(PathMap&& other) noexcept
      : Vector(std::move(other)), compare_(other.compare_) {}
  PathMap& operator=(PathMap&& other) {
    other.swap(*this);
    return *this;
  }

  // inherit these methods from the underlying vector.
  using Vector::begin;
  using Vector::capacity;
  using Vector::cbegin;
  using Vector::cend;
  using Vector::clear;
  using Vector::crbegin;
  using Vector::crend;
  using Vector::empty;
  using Vector::end;
  using Vector::erase;
  using Vector::max_size;
  using Vector::rbegin;
  using Vector::rend;
  using Vector::reserve;
  using Vector::size;

  // Swap contents with another map.
  void swap(PathMap& other) noexcept {
    Vector::swap(other);
    std::swap(compare_, other.compare_);
  }

  /**
   * lower_bound performs the binary search for locating keys.
   *
   * TODO(xavierd): a potential optimization for case sensitive PathMap would
   * be to first perform a case insensitive search, and then fallback to the
   * case sensitive search.
   */
  iterator lower_bound(Piece key) {
    return std::lower_bound(begin(), end(), key, compare_);
  }

  const_iterator lower_bound(Piece key) const {
    return std::lower_bound(begin(), end(), key, compare_);
  }

  /** Find using the Piece representation of a key.
   * Does not allocate a copy of the key string.
   */
  iterator find(Piece key) {
    auto iter = lower_bound(key);
    if (iter != end() && !compare_(key, iter->first)) {
      // Found it
      return iter;
    }
    return end();
  }

  /** Find using the Piece representation of a key.
   * Does not allocate a copy of the key string.
   */
  const_iterator find(Piece key) const {
    auto iter = lower_bound(key);
    if (iter != end() && !compare_(key, iter->first)) {
      // Found it
      return iter;
    }
    return end();
  }

  /** Insert a new key-value pair.
   * If the key already exists, it is left unaltered.
   * Returns a pair consisting of an iterator to the position for key and
   * a boolean that is true if an insert took place. */
  std::pair<iterator, bool> insert(const value_type& val) {
    auto iter = lower_bound(val.first);

    if (iter != end() && !compare_(val.first, iter->first)) {
      // Found it; leave it alone
      return std::make_pair(iter, false);
    }

    // Otherwise, iter is the insertion point
    return std::make_pair(Vector::insert(iter, val), true);
  }

  /** Emplace a new key-value pair by constructing it in-place.
   * If the key already exists, it is left unaltered.
   * If an insertion happens, the args are forwarded to the Value
   * constructor.
   * Returns a pair consisting of an iterator to the position for key and
   * a boolean that is true if an insert took place. */
  template <typename... Args>
  std::pair<iterator, bool> emplace(Piece key, Args&&... args) {
    auto iter = lower_bound(key);

    if (iter != end() && !compare_(key, iter->first)) {
      // Found it; leave it alone
      return std::make_pair(iter, false);
    }

    // Otherwise, iter is the insertion point
    iter = Vector::emplace(
        iter, std::make_pair(Key(key), Value(std::forward<Args>(args)...)));
    return std::make_pair(iter, true);
  }

  /** Returns a reference to the map position for key, creating it needed.
   * If the key is already present, no additional allocations are performed. */
  mapped_type& operator[](Piece key) {
    auto iter = lower_bound(key);

    if (iter != end() && !compare_(key, iter->first)) {
      // Found it
      return iter->second;
    }

    // Not yet present, make a new one at the insertion point
    iter = Vector::insert(iter, std::make_pair(Key(key), mapped_type()));
    return iter->second;
  }

  /** Returns a reference to the map position for key, if present.
   * Throws std::out_of_range if the key is not present (this const
   * form is not allowed to mutate the map). */
  const mapped_type& operator[](Piece key) const {
    return at(key);
  }

  /** Returns a reference to the map position for key, if present.
   * Throws std::out_of_range if the key is not present. */
  mapped_type& at(Piece key) {
    auto iter = find(key);
    if (iter == end()) {
      throwf<std::out_of_range>("no such key {}", key);
    }
    return iter->second;
  }

  /** Returns a reference to the map position for key, if present.
   * Throws std::out_of_range if the key is not present. */
  const mapped_type& at(Piece key) const {
    const auto iter = find(key);
    if (iter == end()) {
      throwf<std::out_of_range>("no such key {}", key);
    }
    return iter->second;
  }

  /** Erase the value associated with key.
   * Does not allocate any additional memory to look up the key.
   * Returns the number of matching elements that were erased; this is
   * always either 1 or 0. */
  size_type erase(Piece key) {
    auto iter = find(key);
    if (iter == end()) {
      return 0;
    }
    erase(iter);
    return 1;
  }

  /** Returns 1 if there is an entry with the given key and 0 otherwise. */
  size_type count(Piece key) const {
    auto iter = find(key);
    return iter != end();
  }

  CaseSensitivity getCaseSensitivity() const {
    return compare_.caseSensitive_;
  }

  /// Equality operator.
  template <typename V, typename K>
  friend bool operator==(const PathMap<V, K>& lhs, const PathMap<V, K>& rhs);

  /// Inequality operator.
  template <typename V, typename K>
  friend bool operator!=(const PathMap<V, K>& lhs, const PathMap<V, K>& rhs);
};

// Implementations of the equality operators; gcc hates us if we
// define them inline in the class above.

/// Equality operator.
template <typename V, typename K>
bool operator==(const PathMap<V, K>& lhs, const PathMap<V, K>& rhs) {
  // reinterpret lhs as the underlying vector type.
  const folly::fbvector<std::pair<K, V>>& vector = lhs;
  return vector == rhs;
}

/// Inequality operator.
template <typename V, typename K>
bool operator!=(const PathMap<V, K>& lhs, const PathMap<V, K>& rhs) {
  // reinterpret lhs as the underlying vector type.
  const folly::fbvector<std::pair<K, V>>& vector = lhs;
  return vector != rhs;
}
} // namespace facebook::eden
