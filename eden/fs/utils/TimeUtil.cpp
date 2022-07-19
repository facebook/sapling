/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/TimeUtil.h"

#include <folly/Format.h>
#include <folly/logging/xlog.h>

namespace facebook::eden {

std::string durationStr(std::chrono::nanoseconds duration) {
  using namespace std::chrono_literals;

  // This code is good enough for our use case of generating human-readable
  // times in log messages.  In the future we could probably be smarter at
  // deciding how much precision to show in the output.

  if (duration < 1us) {
    return fmt::format("{}ns", duration.count());
  } else if (duration < 1ms) {
    return fmt::format("{:.3}us", duration.count() / 1000.0);
  } else if (duration < 1s) {
    return fmt::format("{:.3}ms", duration.count() / 1000000.0);
  } else if (duration < 1min) {
    return fmt::format("{:.3}s", duration.count() / 1000000000.0);
  } else if (duration < 1h) {
    auto minutes = std::chrono::duration_cast<std::chrono::minutes>(duration);
    auto remainder = duration - minutes;
    return fmt::format(
        "{}m{:.3}s", minutes.count(), remainder.count() / 1000000000.0);
  } else if (duration < 24h) {
    auto hours = std::chrono::duration_cast<std::chrono::hours>(duration);
    auto remainder = duration - hours;
    auto minutes = std::chrono::duration_cast<std::chrono::minutes>(remainder);
    remainder -= minutes;
    return fmt::format(
        "{}h{}m{:.3}s",
        hours.count(),
        minutes.count(),
        remainder.count() / 1000000000.0);
  } else {
    using days_type =
        std::chrono::duration<std::chrono::hours::rep, std::ratio<86400>>;

    auto remainder = duration;
    auto days = std::chrono::duration_cast<days_type>(remainder);
    remainder -= days;

    auto hours = std::chrono::duration_cast<std::chrono::hours>(remainder);
    remainder -= hours;

    auto minutes = std::chrono::duration_cast<std::chrono::minutes>(remainder);
    remainder -= minutes;

    return fmt::format(
        "{}d{:02}h{:02}m{:.3}s",
        days.count(),
        hours.count(),
        minutes.count(),
        remainder.count() / 1000000000.0);
  }
}

// Set of all the Comparision operators for comparing two timespec structs.
bool operator<(const timespec& a, const timespec& b) {
  XCHECK_LT(a.tv_nsec, 1000000000);
  XCHECK_LT(b.tv_nsec, 1000000000);
  if (a.tv_sec == b.tv_sec) {
    return a.tv_nsec < b.tv_nsec;
  } else {
    return a.tv_sec < b.tv_sec;
  }
}
bool operator<=(const timespec& a, const timespec& b) {
  XCHECK_LT(a.tv_nsec, 1000000000);
  XCHECK_LT(b.tv_nsec, 1000000000);
  if (a.tv_sec == b.tv_sec) {
    return a.tv_nsec <= b.tv_nsec;
  } else {
    return a.tv_sec < b.tv_sec;
  }
}

std::string formatNsTimeToMs(uint64_t ns) {
  // Convert to microseconds before converting to double in case we have a
  // duration longer than 3 months.
  double d = double(ns / 1000);
  return fmt::format("{:.3f} ms", d / 1000.0);
}

std::string formatMicrosecondTime(long microseconds) {
  if (microseconds < 0) {
    return "";
  }
  if (microseconds < 1000) {
    return fmt::format("{} μs", microseconds);
  }
  if (microseconds < 1000000) {
    return fmt::format("{:.3f} ms", microseconds / 1000.0);
  }
  return fmt::format("{:.3f} s", microseconds / 1000000.0);
}
} // namespace facebook::eden
