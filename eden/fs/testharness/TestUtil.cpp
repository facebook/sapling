/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
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
}
}
