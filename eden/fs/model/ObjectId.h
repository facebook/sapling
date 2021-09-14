/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <string>

namespace facebook::eden {

/**
 * A BackingStore-defined identifier for blobs and trees. For example, in Git,
 * these are 20-byte SHA-1 hashes.
 */
class ObjectId {
 public:
  ObjectId() noexcept = default;
  ObjectId(ObjectId&&) noexcept = default;
  ObjectId& operator=(ObjectId&&) noexcept = default;

  explicit ObjectId(std::string value) noexcept : value_{std::move(value)} {}

  /**
   * Until the migration from Hash to ObjectId is complete, make copies explicit
   * with the copy() member function.
   */
  ObjectId(const ObjectId& that) = delete;

  /**
   * Until the migration from Hash to ObjectId is complete, make copies explicit
   * with the copy() member function.
   */
  ObjectId& operator=(const ObjectId& that) = delete;

  const std::string& value() const noexcept {
    return value_;
  }

 private:
  // TODO: These are small and immutable, so using fbstring or something
  // that doesn't even require a capacity might be beneficial.
  std::string value_;
};

} // namespace facebook::eden
