/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "Tree.h"

namespace facebook {
namespace eden {
bool operator==(const Tree& tree1, const Tree& tree2) {
  return (tree1.getHash() == tree2.getHash()) &&
      (tree1.getTreeEntries() == tree2.getTreeEntries());
}

bool operator!=(const Tree& tree1, const Tree& tree2) {
  return !(tree1 == tree2);
}
} // namespace eden
} // namespace facebook
