/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include <folly/chrono/Conv.h>
#include <stdint.h>
#include <time.h>
#include <chrono>

namespace facebook {
namespace eden {

#ifndef _WIN32

/** Helper for accessing the `atime` field of a `struct stat` as a timespec.
 * Linux and macOS have different names for this field. */
inline const struct timespec& stAtime(const struct stat& st) {
#ifdef __APPLE__
  return st.st_atimespec;
#elif defined(_BSD_SOURCE) || defined(_SVID_SOURCE) || \
    _POSIX_C_SOURCE >= 200809L || _XOPEN_SOURCE >= 700
  return st.st_atim;
#else
#error teach this system how to get the stat change time as a timespec
#endif
}

/** Helper for accessing the `mtime` field of a `struct stat` as a timespec.
 * Linux and macOS have different names for this field. */
inline const struct timespec& stMtime(const struct stat& st) {
#ifdef __APPLE__
  return st.st_mtimespec;
#elif defined(_BSD_SOURCE) || defined(_SVID_SOURCE) || \
    _POSIX_C_SOURCE >= 200809L || _XOPEN_SOURCE >= 700
  return st.st_mtim;
#else
#error teach this system how to get the stat modify time as a timespec
#endif
}

/** Helper for accessing the `ctime` field of a `struct stat` as a timespec.
 * Linux and macOS have different names for this field. */
inline const struct timespec& stCtime(const struct stat& st) {
#ifdef __APPLE__
  return st.st_ctimespec;
#elif defined(_BSD_SOURCE) || defined(_SVID_SOURCE) || \
    _POSIX_C_SOURCE >= 200809L || _XOPEN_SOURCE >= 700
  return st.st_ctim;
#else
#error teach this system how to get the stat change time as a timespec
#endif
}

/**
 * Access stat atime as a system_clock::time_point.
 */
inline std::chrono::system_clock::time_point stAtimepoint(
    const struct stat& st) {
  return folly::to<std::chrono::system_clock::time_point>(stAtime(st));
}

/**
 * Access stat mtime as a system_clock::time_point.
 */
inline std::chrono::system_clock::time_point stCtimepoint(
    const struct stat& st) {
  return folly::to<std::chrono::system_clock::time_point>(stCtime(st));
}

/**
 * Access stat ctime as a system_clock::time_point.
 */
inline std::chrono::system_clock::time_point stMtimepoint(
    const struct stat& st) {
  return folly::to<std::chrono::system_clock::time_point>(stMtime(st));
}

#endif

} // namespace eden
} // namespace facebook
