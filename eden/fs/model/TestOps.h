/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <string>
#include "eden/fs/model/ObjectId.h"
#include "eden/fs/model/Tree.h"

namespace facebook::eden {

/*
 * ObjectId comparison operators for convenient unit tests.
 *
 * This header is not used in EdenFS, because call sites should be explicit
 * about byte-wise comparison or use of BackingStore::compareObjectsById.
 */

inline bool operator==(const ObjectId& lhs, const ObjectId& rhs) {
  return lhs.getBytes() == rhs.getBytes();
}

inline bool operator!=(const ObjectId& lhs, const ObjectId& rhs) {
  return lhs.getBytes() != rhs.getBytes();
}

inline bool operator<(const ObjectId& lhs, const ObjectId& rhs) {
  return lhs.getBytes() < rhs.getBytes();
}

/*
 * Tree comparison operators. These shouldn't be used in the EdenFS daemon.
 */

bool operator==(const TreeEntry& entry1, const TreeEntry& entry2);
bool operator!=(const TreeEntry& entry1, const TreeEntry& entry2);

bool operator==(const Tree& tree1, const Tree& tree2);
bool operator!=(const Tree& tree1, const Tree& tree2);

} // namespace facebook::eden
