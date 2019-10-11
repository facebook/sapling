/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "folly/portability/Windows.h"

#include <iostream>
#include <string>
#include "eden/fs/utils/PathFuncs.h"
#include "userenv.h"

#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/win/utils/UserInfo.h"
#include "folly/portability/Unistd.h"
#include "gtest/gtest.h"

using namespace facebook::eden;

using namespace facebook::eden::detail;
using namespace folly;
using namespace folly::detail;

namespace folly {
namespace detail {
//
// For some reason I am getting linker error for this, so added it here.
//
void ScopeGuardImplBase::warnAboutToCrash() noexcept {
  // Ensure the availability of std::cerr
  std::ios_base::Init ioInit;
  std::cerr
      << "This program will now terminate because a folly::ScopeGuard callback "
         "threw an \nexception.\n";
}
} // namespace detail
} // namespace folly

TEST(UserInfoTest, testUserName) {
  UserInfo user;
  EXPECT_EQ(getenv("USERNAME"), user.getUsername());
}

TEST(UserInfoTest, testHomeDirectory) {
  UserInfo user;
  EXPECT_EQ(realpath(getenv("USERPROFILE")), user.getHomeDirectory());
}
