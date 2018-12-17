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
#include <stdint.h>
#include <time.h>

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

#endif

} // namespace eden
} // namespace facebook
