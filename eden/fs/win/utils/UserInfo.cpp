/*
 *  Copyright (c) 2018-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */

#include "folly/portability/Windows.h"

#include <Lmcons.h>
#include <iostream>
#include <string>
#include "eden/fs/utils/PathFuncs.h"
#include "userenv.h"

#include "UserInfo.h"
#include "eden/fs/win/utils/Handle.h"
#include "eden/fs/win/utils/WinError.h"
using namespace std;
using namespace folly;
using namespace folly::detail;

namespace facebook {
namespace eden {

UserInfo::UserInfo() : username_(UNLEN, 0) {
  DWORD size = username_.size() + 1;
  TokenHandle tokenHandle;

  if (!GetUserNameA(username_.data(), &size)) {
    throw makeWin32ErrorExplicit(GetLastError(), "Failed to get the user name");
  }

  username_.resize(size - 1);

  if (!OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, tokenHandle.set())) {
    throw makeWin32ErrorExplicit(
        GetLastError(), "Failed to get the process token");
  }

  // The profile path could be of any arbitrary length, so if we failed to get
  // with error ERROR_INSUFFICIENT_BUFFER then we retry with the right size
  // buffer.

  size = MAX_PATH;
  string profile(size - 1, 0);
  bool retry = false;

  if (!GetUserProfileDirectoryA(tokenHandle.get(), profile.data(), &size)) {
    if (GetLastError() == ERROR_INSUFFICIENT_BUFFER) {
      retry = true;
    } else {
      throw makeWin32ErrorExplicit(
          GetLastError(), "Failed to get user profile directory");
    }
  }

  profile.resize(size - 1);
  if (retry) {
    if (!GetUserProfileDirectoryA(tokenHandle.get(), profile.data(), &size)) {
      throw makeWin32ErrorExplicit(
          GetLastError(), "Failed to get user profile directory");
    }
    profile.resize(size - 1);
  }

  homeDirectory_ = realpath(profile);
}

} // namespace eden
} // namespace facebook
