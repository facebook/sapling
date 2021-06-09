/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "Tree.h"

namespace facebook::eden {

bool operator==(const Tree& tree1, const Tree& tree2) {
  return (tree1.getHash() == tree2.getHash()) &&
      (tree1.getTreeEntries() == tree2.getTreeEntries());
}

bool operator!=(const Tree& tree1, const Tree& tree2) {
  return !(tree1 == tree2);
}

size_t Tree::getSizeBytes() const {
  // TODO: we should consider using a standard memory framework across
  // eden for this type of thing. D17174143 is one such idea.
  size_t internal_size = sizeof(*this);

  size_t indirect_size =
      folly::goodMallocSize(sizeof(TreeEntry) * entries_.capacity());

  for (auto& entry : entries_) {
    indirect_size += entry.getIndirectSizeBytes();
  }
  return internal_size + indirect_size;
}

} // namespace facebook::eden
