/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/digest/Blake3.h"

#include <stdexcept>

#include <blake3.h> // @manual

namespace facebook::eden {

Blake3::Blake3() {
  blake3_hasher_init(&hasher_);
}

Blake3::Blake3(folly::ByteRange key) {
  if (key.size() != BLAKE3_KEY_LEN) {
    throw std::invalid_argument("Invalid key size, it must be 32 bytes length");
  }

  auto* const keyPtr = key.data();
  const uint8_t(&keyArray)[BLAKE3_KEY_LEN] =
      *reinterpret_cast<const uint8_t(*)[BLAKE3_KEY_LEN]>(keyPtr);
  blake3_hasher_init_keyed(&hasher_, keyArray);
}

/* static */ Blake3 Blake3::create(std::optional<folly::ByteRange> key) {
  return key ? Blake3(*key) : Blake3();
}

/* static */ Blake3 Blake3::create(const std::optional<std::string>& key) {
  return key ? Blake3::create(folly::ByteRange{
                   folly::StringPiece{key->data(), key->size()}})
             : Blake3::create(std::optional<folly::ByteRange>());
}

/* static */ Blake3 Blake3::create(std::optional<folly::StringPiece> key) {
  return key ? Blake3::create(folly::ByteRange{*key})
             : Blake3::create(std::optional<folly::ByteRange>());
}

void Blake3::update(const void* data, size_t size) {
  blake3_hasher_update(&hasher_, data, size);
}

void Blake3::update(folly::ByteRange data) {
  update(data.data(), data.size());
}

void Blake3::update(folly::StringPiece data) {
  update(data.data(), data.size());
}

void Blake3::finalize(folly::MutableByteRange out) {
  if (out.size() != BLAKE3_OUT_LEN) {
    throw std::invalid_argument("Unexpected len");
  }

  blake3_hasher_finalize(&hasher_, out.data(), BLAKE3_OUT_LEN);
}

} // namespace facebook::eden
