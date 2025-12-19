/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */

#ifndef COMMON_NETWORKUTIL_H
#define COMMON_NETWORKUTIL_H 1

#include <arpa/inet.h>
#include <netdb.h>
#include <sys/socket.h>
#include <cstring>
#include <string>

namespace facebook {
namespace network {

/* Network utility functions for OSS build */
class NetworkUtil {
 public:
  // Resolve hostname to IP address
  static std::string getHostByName(
      const std::string& host,
      bool disableIpv6 = false) {
    struct addrinfo hints, *result = nullptr;
    std::memset(&hints, 0, sizeof(hints));
    hints.ai_family = disableIpv6 ? AF_INET : AF_UNSPEC;
    hints.ai_socktype = SOCK_STREAM;

    int status = getaddrinfo(host.c_str(), nullptr, &hints, &result);
    if (status != 0 || result == nullptr) {
      return "";
    }

    char ipstr[INET6_ADDRSTRLEN];
    void* addr = nullptr;

    if (result->ai_family == AF_INET) {
      struct sockaddr_in* ipv4 = (struct sockaddr_in*)result->ai_addr;
      addr = &(ipv4->sin_addr);
    } else if (result->ai_family == AF_INET6) {
      struct sockaddr_in6* ipv6 = (struct sockaddr_in6*)result->ai_addr;
      addr = &(ipv6->sin6_addr);
    }

    std::string ipAddress;
    if (addr != nullptr &&
        inet_ntop(result->ai_family, addr, ipstr, sizeof(ipstr)) != nullptr) {
      ipAddress = ipstr;
    }

    freeaddrinfo(result);
    return ipAddress;
  }

  // Reverse DNS lookup: IP address to hostname
  static std::string getHostByAddr(const std::string& ip) {
    struct sockaddr_in sa4;
    struct sockaddr_in6 sa6;
    struct sockaddr* sa = nullptr;
    socklen_t salen = 0;

    // Try IPv4 first
    if (inet_pton(AF_INET, ip.c_str(), &(sa4.sin_addr)) == 1) {
      sa4.sin_family = AF_INET;
      sa = (struct sockaddr*)&sa4;
      salen = sizeof(sa4);
    } else if (inet_pton(AF_INET6, ip.c_str(), &(sa6.sin6_addr)) == 1) {
      sa6.sin6_family = AF_INET6;
      sa = (struct sockaddr*)&sa6;
      salen = sizeof(sa6);
    } else {
      // Invalid IP address
      return "";
    }

    char hostname[NI_MAXHOST];
    // Use NI_NAMEREQD to ensure we get a name (not just the IP back)
    // and default flags to get FQDN
    int status = getnameinfo(
        sa, salen, hostname, sizeof(hostname), nullptr, 0, NI_NAMEREQD);
    if (status != 0) {
      // If NI_NAMEREQD fails, try without it (will return IP if no name found)
      status =
          getnameinfo(sa, salen, hostname, sizeof(hostname), nullptr, 0, 0);
      if (status != 0) {
        return "";
      }
    }

    return std::string(hostname);
  }
};
} // namespace network
} // namespace facebook

#endif // #ifndef COMMON_NETWORKUTIL_H
