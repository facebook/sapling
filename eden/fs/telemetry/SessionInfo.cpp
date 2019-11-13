/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/SessionInfo.h"
#include <folly/Exception.h>

#if defined(__linux__) || defined(__APPLE__)
#include <sys/utsname.h>
#include <unistd.h>
#endif

#if defined(_WIN32)
#include <winsock.h> // @manual
#endif

namespace {
/**
 * Windows limits hostnames to 256 bytes. Linux provides HOST_NAME_MAX
 * and MAXHOSTNAMELEN constants, defined as 64. Both Linux and macOS
 * define _POSIX_HOST_NAME_MAX as 256.  Both Linux and macOS allow
 * reading the host name limit at runtime with
 * sysconf(_SC_HOST_NAME_MAX).
 *
 * RFC 1034 limits complete domain names to 255:
 * https://tools.ietf.org/html/rfc1034#section-3.1
 * > To simplify implementations, the total number of octets that represent a
 * > domain name (i.e., the sum of all label octets and label lengths) is
 * > limited to 255.
 *
 * Rather than querying dynamically or selecting a constant based on platform,
 * assume 256 is sufficient everywhere.
 */
constexpr size_t kHostNameMax = 256;
} // namespace

namespace facebook {
namespace eden {

std::string getOperatingSystemName() {
#if defined(_WIN32)
  return "Windows";
#elif defined(__linux__)
  return "Linux";
#elif defined(__APPLE__)
  // Presuming EdenFS doesn't run on iOS, watchOS, or tvOS. :)
  return "macOS";
#else
  return "unknown";
#endif
}

std::string getOperatingSystemVersion() {
#if defined(_WIN32)
  // TODO: Implement build version lookup, e.g. 1903
  // reg query "HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion" /v releaseid
  return "10";
#elif defined(__linux__) || defined(__APPLE__)
  struct utsname uts;
  if (uname(&uts)) {
    return "error";
  }
  return uts.release;
#else
  return "unknown";
#endif
}

std::string getHostname() {
  char hostname[kHostNameMax + 1];
  folly::checkUnixError(
      gethostname(hostname, sizeof(hostname)),
      "gethostname() failed, errno: ",
      errno);

  // POSIX does not require implementations of gethostname to
  // null-terminate. Ensure null-termination after the call.
  hostname[kHostNameMax] = 0;

  return hostname;
}

} // namespace eden
} // namespace facebook
