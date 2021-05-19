/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */

#ifndef FACEBOOK_PROCESS_PROCESS_H_
#define FACEBOOK_PROCESS_PROCESS_H_

#include <string>

namespace facebook {
namespace process {
class Process {
 public:
  static bool execShellCmd(
      const std::string& cmd,
      std::string* out,
      std::string* err,
      int64_t timeoutMsecs = 0) {
          return true;
      }
};

} // namespace process
} // namespace facebook

#endif // FACEBOOK_PROCESS_PROCESS_H_
