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

/**
 * Comparision operators for comparing two timespec structs.
 */
bool operator<(const timespec& a, const timespec& b);
bool operator<=(const timespec& a, const timespec& b);
inline bool operator>=(const timespec& a, const timespec& b) {
  return !(b < a);
}
inline bool operator>(const timespec& a, const timespec& b) {
  return !(b <= a);
}
inline bool operator==(const timespec& a, const timespec& b) {
  return (a.tv_sec == b.tv_sec) && (a.tv_nsec == b.tv_nsec);
}
inline bool operator!=(const timespec& a, const timespec& b) {
  return !(b == a);
}
}
}
