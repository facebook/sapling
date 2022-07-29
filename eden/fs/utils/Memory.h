/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include <folly/FBString.h>
#include <map>
#include <string>

namespace facebook::eden {

/**
 * Asserts the specified memory consists entirely of zeroes, and aborts the
 * process if not.
 */
void assertZeroBits(const void* memory, size_t size);

template <typename T>
void assertZeroBits(const T& value) {
  assertZeroBits(&value, sizeof(T));
}

template <typename StringType>
bool isStringStorageEmbedded(const StringType& t) {
  const void* tbegin = &t;
  const void* tend = &t + 1;
  return std::less_equal<const void*>{}(tbegin, t.data()) &&
      std::less<const void*>{}(t.data(), tend);
}

template <typename StringType>
size_t estimateIndirectMemoryUsage(const StringType& s) {
  if (isStringStorageEmbedded(s)) {
    return 0;
  } else {
    return folly::goodMallocSize(s.capacity());
  }
}

template <typename KeyType, typename ValueType>
size_t estimateIndirectMemoryUsage(
    const std::map<KeyType, ValueType>& entries) {
  // std::map is implemented using a red-black tree.

  // Accumulate the estimated usage of the base nodes of the tree
#if defined(_STL_TREE_H)
  size_t usage = folly::goodMallocSize(sizeof(
                     std::_Rb_tree_node<std::pair<const KeyType, ValueType>>)) *
      entries.size();
#elif defined(_XTREE_)
  size_t usage =
      folly::goodMallocSize(
          sizeof(std::_Tree_node<std::pair<const KeyType, ValueType>, void*>)) *
      entries.size();
#elif defined(_LIBCPP___TREE)
  size_t usage =
      folly::goodMallocSize(sizeof(
          std::__tree_node<std::pair<const KeyType, ValueType>, void*>)) *
      entries.size();
#endif

  // Accumulate any indirect usage from the nodes
  for (const auto& pair : entries) {
    usage += estimateIndirectMemoryUsage(std::get<0>(pair));
    if (auto* entryHash = std::get<1>(pair).get_hash()) {
      usage += estimateIndirectMemoryUsage(*entryHash);
    }
  }

  return usage;
}

} // namespace facebook::eden
