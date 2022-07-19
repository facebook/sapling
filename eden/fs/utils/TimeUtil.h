/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <chrono>
#include <string>

namespace facebook::eden {

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

/**
 * Specifically converts a duration of nanoseconds into milliseconds and
 * returns a string in human readable format.
 */
std::string formatNsTimeToMs(uint64_t ns);

/**
 * Get a human readable string for a time duration of microseconds in either
 * microsecond, millisecond, or second time. Returns an empty string for
 * negative durations
 */
std::string formatMicrosecondTime(long microseconds);
} // namespace facebook::eden
