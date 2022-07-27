/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/model/TestOps.h"

namespace facebook::eden {

bool operator==(const TreeEntry& entry1, const TreeEntry& entry2) {
  return (entry1.getHash() == entry2.getHash()) &&
      (entry1.getType() == entry2.getType());
}

bool operator!=(const TreeEntry& entry1, const TreeEntry& entry2) {
  return !(entry1 == entry2);
}

bool operator==(const Tree& tree1, const Tree& tree2) {
  return (tree1.getHash() == tree2.getHash()) &&
      (tree1.entries_ == tree2.entries_);
}

bool operator!=(const Tree& tree1, const Tree& tree2) {
  return !(tree1 == tree2);
}

} // namespace facebook::eden
