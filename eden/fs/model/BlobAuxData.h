/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <cstdint>
#include <optional>

#include "eden/fs/model/BlobAuxDataFwd.h"
#include "eden/fs/model/Hash.h"

namespace facebook::eden {

/**
 * A small struct containing both the size, the SHA-1 hash and Blake3 hash of
 * a Blob's contents.
 */
class BlobAuxData {
 public:
  BlobAuxData(Hash20 sha1, Hash32 blake3, uint64_t fileLength)
      : sha1(std::move(sha1)), blake3(std::move(blake3)), size(fileLength) {}

  BlobAuxData(Hash20 sha1, std::optional<Hash32> blake3, uint64_t fileLength)
      : sha1(std::move(sha1)), blake3(std::move(blake3)), size(fileLength) {}

  Hash20 sha1;
  // TODO: make it non optional
  std::optional<Hash32> blake3;
  uint64_t size;
};

} // namespace facebook::eden
