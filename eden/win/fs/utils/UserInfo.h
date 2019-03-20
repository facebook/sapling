/*
 *  Copyright (c) 2018-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */

#pragma once
#include <string>
#include "eden/fs/utils/PathFuncs.h"
#include "eden/win/fs/utils/Stub.h"

namespace facebook {
namespace eden {

class UserInfo {
 public:
  UserInfo();

  const std::string& getUsername() const {
    return username_;
  }

  const AbsolutePath& getHomeDirectory() const {
    return homeDirectory_;
  }

  uid_t getUid() const {
    return uid_;
  }

 private:
  std::string username_;
  AbsolutePath homeDirectory_;

  // TODO(puneetk): This hardcode might not hurt us in short run given we only
  // support single user on a windows machine. We should fix this in the long
  // run though.
  uid_t uid_ = 9999999;
};

} // namespace eden
} // namespace facebook
