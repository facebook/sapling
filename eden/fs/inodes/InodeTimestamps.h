/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/utils/TimeUtil.h"

#include <stdint.h>
#include <time.h>

struct stat;

namespace facebook::eden {

class Clock;
struct DesiredMetadata;

/**
 * For space efficiency, store timestamps in a single 64-bit value as
 * nanoseconds from 1901-12-13 (-0x80000000 seconds before unix epoch) through
 * 2446.  This range is similar to ext4's timestamp range, though slightly
 * larger.
 *
 * https://ext4.wiki.kernel.org/index.php/Ext4_Disk_Layout#Inode_Timestamps
 */
class EdenTimestamp {
 public:
  constexpr static struct Clamp {
  } clamp{};
  constexpr static struct ThrowIfOutOfRange {
  } throwIfOutOfRange{};

  /**
   * Default construction produces a timestamp at EdenTimestamp's earliest
   * representable value.
   */
  EdenTimestamp() = default;

  EdenTimestamp(const EdenTimestamp&) = default;

  /**
   * Constructs an EdenTimestamp given a raw uint64_t in nanoseconds since
   * the earliest representable ext4 timestamp.
   */
  explicit EdenTimestamp(uint64_t nsec) noexcept : nsec_(nsec) {}

  /**
   * Converts a timespec to an EdenTimestamp.
   *
   * If the timespec is out of range, it is clamped.
   */
  explicit EdenTimestamp(timespec ts, Clamp = clamp) noexcept;

  /**
   * Converts a timespec to an EdenTimestamp.
   *
   * If the timespec is out of range, std::overflow_error or
   * std::underflow_error is thrown.
   */
  EdenTimestamp(timespec ts, ThrowIfOutOfRange);

  EdenTimestamp& operator=(const EdenTimestamp&) = default;

  EdenTimestamp& operator=(timespec ts) noexcept {
    return *this = EdenTimestamp{ts};
  }

  bool operator==(EdenTimestamp ts) const {
    return nsec_ == ts.nsec_;
  }

  bool operator<(EdenTimestamp ts) const {
    return nsec_ < ts.nsec_;
  }

  /**
   * Returns a timespec representing duration since the unix epoch.
   */
  timespec toTimespec() const noexcept;

  /**
   * Returns the raw representation -- should be for testing only.  :)
   */
  uint64_t asRawRepresentation() const noexcept {
    return nsec_;
  }

 private:
  uint64_t nsec_{0};
};

inline bool operator!=(EdenTimestamp lhs, EdenTimestamp rhs) {
  return !(lhs == rhs);
}

inline bool operator==(EdenTimestamp lhs, timespec rhs) {
  // Widen before comparing.
  return lhs.toTimespec() == rhs;
}

inline bool operator==(timespec lhs, EdenTimestamp rhs) {
  // Widen before comparing.
  return lhs == rhs.toTimespec();
}

inline bool operator<(EdenTimestamp lhs, timespec rhs) {
  // Widen before comparing.
  return lhs.toTimespec() < rhs;
}

inline bool operator<(timespec lhs, EdenTimestamp rhs) {
  // Widen before comparing.
  return lhs < rhs.toTimespec();
}

/**
 * Structure for wrapping atime,ctime,mtime
 */
struct InodeTimestamps {
  EdenTimestamp atime{};
  EdenTimestamp mtime{};
  EdenTimestamp ctime{};

  /**
   * Initializes all timestamps to zero.
   */
  InodeTimestamps() = default;

  /**
   * Initializes all timestamps from the same value.
   */
  explicit InodeTimestamps(const EdenTimestamp time) noexcept
      : atime{time}, mtime{time}, ctime{time} {}

  /**
   * Assigns the specified ts to atime, mtime, and ctime.
   */
  void setAll(const timespec& ts) noexcept {
    atime = ts;
    mtime = ts;
    ctime = ts;
  }

  // Comparison operator for testing purposes
  bool operator==(const InodeTimestamps& other) const noexcept {
    return atime == other.atime && mtime == other.mtime && ctime == other.ctime;
  }

#ifndef _WIN32
  /**
   * Helper that assigns all three timestamps from the flags and parameters in
   * a DesiredMetadata struct.
   *
   * Always sets ctime to the current time as given by the clock.
   */
  void setattrTimes(const Clock& clock, const DesiredMetadata& attr);

  /**
   * Updates st_atime, st_mtime, and st_ctime of the given stat struct.
   */
  void applyToStat(struct stat& st) const;
#endif
};

static_assert(noexcept(EdenTimestamp{}), "");
static_assert(noexcept(EdenTimestamp{timespec{}}), "");
static_assert(noexcept(EdenTimestamp{uint64_t{}}), "");

static_assert(noexcept(InodeTimestamps{}), "");

} // namespace facebook::eden
