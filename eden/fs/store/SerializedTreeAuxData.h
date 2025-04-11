/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <optional>

#include <folly/Range.h>

#include "eden/fs/model/Hash.h"
#include "eden/fs/model/ObjectId.h"
#include "eden/fs/model/TreeAuxData.h"
#include "eden/fs/store/StoreResult.h"

namespace facebook::eden {

class SerializedTreeAuxData {
 public:
  explicit SerializedTreeAuxData(const TreeAuxData& treeAuxData);
  SerializedTreeAuxData(
      const std::optional<Hash32>& digestHash,
      uint64_t digestSize);
  folly::ByteRange slice() const;
  static constexpr size_t SIZE = sizeof(uint64_t) + Hash32::RAW_SIZE;

  static TreeAuxDataPtr parse(
      const ObjectId& treeID,
      const StoreResult& result);

 private:
  void serialize(const std::optional<Hash32>& digestHash, uint64_t digestSize);

  /**
   * Bytes and the size of the data.
   * The components of the bytes are:
   * - version (1 byte)
   * - digest_size (varint, little endian)
   * - used_hashes (varint, little endian)
   * - hashes (N bytes)
   */
  std::pair<std::unique_ptr<uint8_t[]>, size_t> dataAndSize_;
};

} // namespace facebook::eden
