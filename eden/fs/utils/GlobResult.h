/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/model/RootId.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/utils/DirType.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook::eden {

struct GlobResult {
  RelativePath name;
  dtype_t dtype;
  // Currently this is the commit hash for the commit to which this file
  // belongs. But should eden move away from commit hashes this may become
  // the tree hash of the root tree to which this file belongs.
  // This should never become a dangling reference because the caller
  // of Globresult::evaluate ensures that the hashes have a lifetime that
  // exceeds that of the GlobResults returned.
  const RootId* originHash;

  // Comparison operator for testing purposes
  bool operator==(const GlobResult& other) const noexcept {
    return name == other.name && dtype == other.dtype &&
        originHash == other.originHash;
  }
  bool operator!=(const GlobResult& other) const noexcept {
    return !(*this == other);
  }

  bool operator<(const GlobResult& other) const noexcept {
    return name < other.name || (name == other.name && dtype < other.dtype) ||
        (name == other.name && dtype == other.dtype &&
         originHash < other.originHash);
  }

  // originHash should never become a dangling refernece because the caller
  // of Globresult::evaluate ensures that the hashes have a lifetime that
  // exceeds that of the GlobResults returned.
  GlobResult(RelativePathPiece name, dtype_t dtype, const RootId& originHash)
      : name(name.copy()), dtype(dtype), originHash(&originHash) {}

  GlobResult(
      RelativePath&& name,
      dtype_t dtype,
      const RootId& originHash) noexcept
      : name(std::move(name)), dtype(dtype), originHash(&originHash) {}
};

using ResultList = folly::Synchronized<std::vector<GlobResult>>;

using PrefetchList = folly::Synchronized<std::vector<ObjectId>>;

} // namespace facebook::eden
