/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/LinuxKernelVersion.h"

#include <cerrno>
#include <cstdio>
#include <stdexcept>
#include <string>
#include <system_error>

#ifdef __linux__
#include <sys/utsname.h>
#endif

namespace facebook::eden {

LinuxKernelVersion parseLinuxKernelVersion(folly::StringPiece release) {
  unsigned major;
  unsigned minor;
  if (sscanf(release.str().c_str(), "%u.%u", &major, &minor) != 2) {
    throw std::invalid_argument("invalid Linux kernel release");
  }
  return LinuxKernelVersion{
      static_cast<uint32_t>(major), static_cast<uint32_t>(minor)};
}

LinuxKernelVersion getRunningLinuxKernelVersion() {
#ifdef __linux__
  struct utsname name{};
  if (uname(&name) != 0) {
    throw std::system_error(
        errno,
        std::generic_category(),
        "failed to inspect Linux kernel version");
  }
  return parseLinuxKernelVersion(name.release);
#else
  throw std::runtime_error("Linux kernel version is only available on Linux");
#endif
}

} // namespace facebook::eden
