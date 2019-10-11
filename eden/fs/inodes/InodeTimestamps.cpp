/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/InodeTimestamps.h"

#include <folly/Conv.h>
#include <sys/stat.h>
#include "eden/fs/fuse/FuseTypes.h"
#include "eden/fs/utils/Clock.h"

namespace facebook {
namespace eden {

namespace {
/**
 * Like ext4, our earliest representable date is 2^31 seconds before the unix
 * epoch, which works out to December 13th, 1901.
 */
constexpr int64_t kEpochOffsetSeconds = 0x80000000ll;

/**
 * Largest representable sec,nsec pair.
 *
 * $ python3
 * >>> kEpochOffsetSeconds = 0x80000000
 * >>> kLargestRepresentableSec = 16299260425
 * >>> kLargestRepresentableNsec = 709551615
 * >>> hex((kEpochOffsetSeconds + kLargestRepresentableSec) * 1000000000 + \
 * ... kLargestRepresentableNsec)
 * '0xffffffffffffffff'
 */
constexpr int64_t kLargestRepresentableSec = 16299260425ll;
constexpr uint32_t kLargestRepresentableNsec = 709551615u;

struct ClampPolicy {
  static constexpr bool is_noexcept = true;
  static uint64_t minimum(timespec /*ts*/) noexcept {
    return 0;
  }
  static uint64_t maximum(timespec /*ts*/) noexcept {
    return ~0ull;
  }
};

struct ThrowPolicy {
  static constexpr bool is_noexcept = false;
  static uint64_t minimum(timespec ts) {
    throw std::underflow_error(folly::to<std::string>(
        "underflow converting timespec (",
        ts.tv_sec,
        " s, ",
        ts.tv_nsec,
        " ns) to EdenTimestamp"));
  }
  static uint64_t maximum(timespec ts) {
    throw std::overflow_error(folly::to<std::string>(
        "overflow converting timespec (",
        ts.tv_sec,
        " s, ",
        ts.tv_nsec,
        " ns) to EdenTimestamp"));
  }
};

template <typename OutOfRangePolicy>
uint64_t repFromTimespec(timespec ts) noexcept(OutOfRangePolicy::is_noexcept) {
  if (ts.tv_sec < -kEpochOffsetSeconds) {
    return OutOfRangePolicy::minimum(ts);
  } else if (
      ts.tv_sec > kLargestRepresentableSec ||
      (ts.tv_sec == kLargestRepresentableSec &&
       ts.tv_nsec > kLargestRepresentableNsec)) {
    return OutOfRangePolicy::maximum(ts);
  } else {
    // Assume that ts.tv_nsec is within [0, 1000000000).
    // The first addition must be unsigned to avoid UB.
    return (static_cast<uint64_t>(kEpochOffsetSeconds) +
            static_cast<uint64_t>(ts.tv_sec)) *
        1000000000ll +
        ts.tv_nsec;
  }
}

timespec repToTimespec(uint64_t nsec) {
  static constexpr uint64_t kEpochNsec = kEpochOffsetSeconds * 1000000000ull;
  if (nsec < kEpochNsec) {
    int64_t before_epoch = kEpochNsec - nsec;
    timespec ts;
    auto sec = (before_epoch + 999999999) / 1000000000;
    ts.tv_sec = -sec;
    ts.tv_nsec = sec * 1000000000 - before_epoch;
    return ts;
  } else {
    uint64_t after_epoch = nsec - kEpochNsec;
    timespec ts;
    ts.tv_sec = after_epoch / 1000000000;
    ts.tv_nsec = after_epoch % 1000000000;
    return ts;
  }
}

} // namespace

EdenTimestamp::EdenTimestamp(timespec ts, Clamp) noexcept
    : nsec_{repFromTimespec<ClampPolicy>(ts)} {}

EdenTimestamp::EdenTimestamp(timespec ts, ThrowIfOutOfRange)
    : nsec_{repFromTimespec<ThrowPolicy>(ts)} {}

timespec EdenTimestamp::toTimespec() const noexcept {
  return repToTimespec(nsec_);
}

void InodeTimestamps::setattrTimes(
    const Clock& clock,
    const fuse_setattr_in& attr) {
  const auto now = clock.getRealtime();

  // Set atime for TreeInode.
  if (attr.valid & FATTR_ATIME) {
    timespec attr_atime;
    attr_atime.tv_sec = attr.atime;
    attr_atime.tv_nsec = attr.atimensec;
    atime = attr_atime;
  } else if (attr.valid & FATTR_ATIME_NOW) {
    atime = now;
  }

  // Set mtime for TreeInode.
  if (attr.valid & FATTR_MTIME) {
    timespec attr_mtime;
    attr_mtime.tv_sec = attr.mtime;
    attr_mtime.tv_nsec = attr.mtimensec;
    mtime = attr_mtime;
  } else if (attr.valid & FATTR_MTIME_NOW) {
    mtime = now;
  }

  // we do not allow users to set ctime using setattr. ctime should be changed
  // when ever setattr is called, as this function is called in setattr, update
  // ctime to now.
  ctime = now;
}

void InodeTimestamps::applyToStat(struct stat& st) const {
#ifdef __APPLE__
  st.st_atimespec = atime.toTimespec();
  st.st_ctimespec = ctime.toTimespec();
  st.st_mtimespec = mtime.toTimespec();
#elif defined(_BSD_SOURCE) || defined(_SVID_SOURCE) || \
    _POSIX_C_SOURCE >= 200809L || _XOPEN_SOURCE >= 700
  st.st_atim = atime.toTimespec();
  st.st_ctim = ctime.toTimespec();
  st.st_mtim = mtime.toTimespec();
#else
  st.st_atime = atime.toTimespec().tv_sec;
  st.st_mtime = mtime.toTimespec().tv_sec;
  st.st_ctime = ctime.toTimespec().tv_sec;
#endif
}

} // namespace eden
} // namespace facebook
