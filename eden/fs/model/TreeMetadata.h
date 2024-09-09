/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <cstdint>
#include <optional>

#include "eden/fs/model/Hash.h"
#include "eden/fs/model/TreeMetadataFwd.h"

namespace facebook::eden {

/**
 * A small struct containing both the size (in bytes) and Blake3 hash of
 * a Tree's RE CAS digest. Note: digest_size != stat(tree).st_size
 */
class TreeMetadata {
 public:
  TreeMetadata(Hash32 blake3, uint64_t digestSize)
      : blake3(std::move(blake3)), digestSize(digestSize) {}

  TreeMetadata(std::optional<Hash32> blake3, uint64_t digestSize)
      : blake3(std::move(blake3)), digestSize(digestSize) {}

  // TODO: make it non optional
  std::optional<Hash32> blake3;
  uint64_t digestSize;
};

} // namespace facebook::eden
