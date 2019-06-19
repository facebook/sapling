/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once
#include <string>
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/win/utils/Stub.h"

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
