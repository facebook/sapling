/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "TestUtil.h"

#include <cstring>
#include <stdexcept>
#include "eden/fs/model/Hash.h"

namespace facebook {
namespace eden {
Hash makeTestHash(folly::StringPiece value) {
  constexpr size_t ASCII_SIZE = 2 * Hash::RAW_SIZE;
  if (value.size() > ASCII_SIZE) {
    throw std::invalid_argument(value.toString() + " is too big for Hash");
  }
  std::array<char, ASCII_SIZE> fullValue;
  memset(fullValue.data(), '0', fullValue.size());
  memcpy(
      fullValue.data() + fullValue.size() - value.size(),
      value.data(),
      value.size());
  return Hash{folly::StringPiece{folly::range(fullValue)}};
}
} // namespace eden
} // namespace facebook
