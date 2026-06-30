/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/model/Tree.h"

#include <folly/memory/Malloc.h>

#include "eden/fs/model/TreeAuxData.h"

namespace facebook::eden {

TreePtr Tree::withNewId(container entries, ObjectId newId) const {
  if (isRestricted()) {
    return std::make_shared<const Tree>(
        Restricted{}, std::move(entries), std::move(newId));
  }
  return std::make_shared<const Tree>(
      std::move(newId), std::move(entries), auxData_, aclRootState_);
}

TreePtr Tree::withNewId(ObjectId newId) const {
  return withNewId(entries_, std::move(newId));
}

size_t Tree::getSizeBytes() const {
  // TODO: we should consider using a standard memory framework across
  // eden for this type of thing. D17174143 is one such idea.
  size_t internal_size = sizeof(*this);

  size_t indirect_size =
      folly::goodMallocSize(sizeof(TreeEntry) * entries_.capacity());

  for (auto& entry : entries_) {
    indirect_size += estimateIndirectMemoryUsage(entry.first.value());
  }

  size_t auxDataSize = 0;
  if (auxData_) {
    auxDataSize = sizeof(uint64_t) +
        (auxData_->digestHash.has_value() ? Hash32::RAW_SIZE : 0);
  }
  return internal_size + indirect_size + auxDataSize;
}

} // namespace facebook::eden
