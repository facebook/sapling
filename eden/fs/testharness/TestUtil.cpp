/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "TestUtil.h"

#include <cstring>
#include <stdexcept>
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/ObjectId.h"

namespace facebook::eden {
ObjectId makeTestHash(folly::StringPiece value) {
  constexpr size_t ASCII_SIZE = 2 * Hash20::RAW_SIZE;
  if (value.size() > ASCII_SIZE) {
    throw std::invalid_argument(value.toString() + " is too big for Hash");
  }
  std::array<char, ASCII_SIZE> fullValue;
  memset(fullValue.data(), '0', fullValue.size());
  memcpy(
      fullValue.data() + fullValue.size() - value.size(),
      value.data(),
      value.size());
  return ObjectId::fromHex(fullValue);
}

Hash20 makeTestHash20(folly::StringPiece value) {
  constexpr size_t ASCII_SIZE = 2 * Hash20::RAW_SIZE;
  if (value.size() > ASCII_SIZE) {
    throw std::invalid_argument(value.toString() + " is too big for Hash");
  }
  std::array<char, ASCII_SIZE> fullValue;
  memset(fullValue.data(), '0', fullValue.size());
  memcpy(
      fullValue.data() + fullValue.size() - value.size(),
      value.data(),
      value.size());
  return Hash20{folly::StringPiece{folly::range(fullValue)}};
}

bool isInodeMaterializedInBuffer(ActivityBuffer& buff, InodeNumber ino) {
  auto events = buff.getAllEvents();
  int num_starts = 0;
  int num_ends = 0;
  for (auto const& event : events) {
    if (event.ino.getRawValue() == ino.getRawValue() &&
        event.eventType == InodeEventType::MATERIALIZE) {
      if (event.progress == InodeEventProgress::START && num_starts == 0) {
        num_starts++;
      } else if (event.progress == InodeEventProgress::END && num_ends == 0) {
        num_ends++;
      } else { // Return early if there exists more than one START or END event
        return false;
      }
    }
  }
  return num_starts == 1 && num_ends == 1;
}
} // namespace facebook::eden
