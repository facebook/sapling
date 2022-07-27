/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/io/IOBuf.h>
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/utils/CaseSensitivity.h"
#include "eden/fs/utils/PathMap.h"

namespace facebook::eden {

class Tree {
 public:
  using container = PathMap<TreeEntry>;
  using key_type = container::key_type;
  using mapped_type = container::mapped_type;
  using value_type = container::value_type;
  using const_iterator = container::const_iterator;

  /**
   * Construct a Tree.
   *
   * Temporarily takes a CaseSensitivity argument default initialized. In the
   * future, once all the callers are updated to pass the correct
   * CaseSensitivity, the default value will be removed.
   *
   * In the case where kPathMapDefaultCaseSensitive is not the same as the
   * mount case sensitivity, the caller is responsible for constructing a new
   * Tree with the case sensitivity flipped.
   */
  explicit Tree(container entries, ObjectId hash)
      : hash_{std::move(hash)}, entries_{std::move(entries)} {}

  const ObjectId& getHash() const {
    return hash_;
  }

  /**
   * An estimate of the memory footprint of this tree. Called by ObjectCache to
   * limit the number of cached trees in memory at a time.
   */
  size_t getSizeBytes() const;

  /**
   * Find an entry in this Tree whose name match the passed in path.
   */
  const_iterator find(PathComponentPiece name) const {
    return entries_.find(name);
  }

  const_iterator cbegin() const {
    return entries_.cbegin();
  }

  const_iterator begin() const {
    return cbegin();
  }

  const_iterator cend() const {
    return entries_.cend();
  }

  const_iterator end() const {
    return cend();
  }

  size_t size() const {
    return entries_.size();
  }

  /**
   * Returns the case sensitivity of this tree.
   */
  CaseSensitivity getCaseSensitivity() const {
    return entries_.getCaseSensitivity();
  }

  /**
   * Serialize tree using custom format.
   */
  folly::IOBuf serialize() const;

  /**
   * Deserialize tree if possible.
   * Returns nullptr if serialization format is not supported.
   *
   * First byte is used to identify serialization format.
   * Git tree starts with 'tree', so we can use any bytes other then 't' as a
   * version identifier. Currently only V1_VERSION is supported, along with
   * git tree format.
   */
  static std::unique_ptr<Tree> tryDeserialize(
      ObjectId hash,
      folly::StringPiece data);

 private:
  friend bool operator==(const Tree& tree1, const Tree& tree2);

  ObjectId hash_;
  container entries_;

  static constexpr uint32_t V1_VERSION = 1u;
};

} // namespace facebook::eden
