/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <chrono>
#include <string>

namespace facebook {
namespace eden {

/**
 * Get a human-readable string for a time duration.
 *
 * Example return values:
 *   3ns
 *   10.456ms
 *   1d23h3500.123s
 */
std::string durationStr(std::chrono::nanoseconds duration);
}
}
