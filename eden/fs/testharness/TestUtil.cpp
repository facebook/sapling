/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/testharness/TestUtil.h"

#include <cstring>
#include <stdexcept>
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/ObjectId.h"

using namespace std::chrono_literals;

namespace facebook::eden {
constexpr auto materializationTimeoutLimit = 1000ms;

ObjectId makeTestId(folly::StringPiece value) {
  constexpr size_t ASCII_SIZE = 2 * Hash20::RAW_SIZE;
  if (value.size() > ASCII_SIZE) {
    throw std::invalid_argument(value.toString() + " is too big for ObjectId");
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
    throw std::invalid_argument(value.toString() + " is too big for Hash20");
  }
  std::array<char, ASCII_SIZE> fullValue;
  memset(fullValue.data(), '0', fullValue.size());
  memcpy(
      fullValue.data() + fullValue.size() - value.size(),
      value.data(),
      value.size());
  return Hash20{folly::StringPiece{folly::range(fullValue)}};
}

bool isInodeMaterializedInQueue(
    folly::UnboundedQueue<InodeTraceEvent, true, true, false>&
        materializationQueue,
    InodeEventProgress progress,
    InodeNumber ino) {
  auto event =
      materializationQueue.try_dequeue_for(materializationTimeoutLimit);
  return event.has_value() && event->progress == progress &&
      event->ino.getRawValue() == ino.getRawValue();
}
} // namespace facebook::eden
