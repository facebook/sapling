/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <optional>

#include <folly/Range.h>

#include <blake3.h>

namespace facebook::eden {

struct Blake3 final {
  Blake3();

  /*
   * Initialize a blake3_hasher in the keyed hashing mode. The key must be
   * exactly 32 bytes.
   * It mostly used for security purposes to make it harder creating a rainbow
   * table in the future
   */
  explicit Blake3(folly::ByteRange key);

  static Blake3 create(std::optional<folly::ByteRange> key);
  static Blake3 create(const std::optional<std::string>& key);
  static Blake3 create(std::optional<folly::StringPiece> key);

  void update(const void* data, size_t size);
  void update(folly::ByteRange data);
  void update(folly::StringPiece data);

  void finalize(folly::MutableByteRange out);

 private:
  blake3_hasher hasher_;
};

} // namespace facebook::eden
