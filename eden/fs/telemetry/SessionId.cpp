/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/SessionId.h"
#include <random>

namespace {

uint32_t generateSessionId() {
  std::random_device rd;
  std::uniform_int_distribution<uint32_t> u;
  return u(rd);
}

} // namespace

namespace facebook::eden {

uint32_t getSessionId() {
  static auto sessionId = generateSessionId();
  return sessionId;
}

} // namespace facebook::eden
