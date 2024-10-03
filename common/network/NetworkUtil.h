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

namespace facebook {
namespace network {

/* Stub util for OSS build */
class NetworkUtil {
 public:
  static std::string getHostByName(
      const std::string& host,
      bool disableIpv6 = false) {
    return "";
  }
  static std::string getHostByAddr(const std::string& ip) {
    return "";
  }
};
} // namespace network
} // namespace facebook

#endif // #ifndef COMMON_NETWORKUTIL_H
