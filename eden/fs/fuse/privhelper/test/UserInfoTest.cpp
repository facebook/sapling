/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/fuse/privhelper/UserInfo.h"

#include <folly/test/TestUtils.h>
#include <gtest/gtest.h>

#include "eden/fs/utils/test/ScopedEnvVar.h"

namespace facebook {
namespace eden {

TEST(UserInfo, initFromSudo) {
  ScopedEnvVar homeVar{"HOME"};
  ScopedEnvVar sudoUidVar{"SUDO_UID"};
  ScopedEnvVar sudoGidVar{"SUDO_GID"};
  ScopedEnvVar sudoUserVar{"SUDO_USER"};

  sudoUidVar.unset();
  sudoGidVar.unset();
  sudoUserVar.unset();

  // initFromSudo() should return false when no SUDO_* variables are defined.
  UserInfo info;
  EXPECT_FALSE(info.initFromSudo());

  // If SUDO_GID is defined without SUDO_GID or SUDO_USER,
  // initFromSudo should throw
  sudoUidVar.set("65534");
  EXPECT_THROW_RE(
      info.initFromSudo(), std::runtime_error, "SUDO_UID set without SUDO_GID");
  sudoGidVar.set("65534");
  EXPECT_THROW_RE(
      info.initFromSudo(),
      std::runtime_error,
      "SUDO_UID set without SUDO_USER");

  // If SUDO_UID or SUDO_GID is bogus, initFromSudo should throw
  sudoUidVar.set("");
  sudoGidVar.set("65534");
  sudoUserVar.set("some_test_user");
  EXPECT_THROW_RE(
      info.initFromSudo(), std::runtime_error, "invalid value for SUDO_UID: ");
  sudoUidVar.set("asdf");
  EXPECT_THROW_RE(
      info.initFromSudo(),
      std::runtime_error,
      "invalid value for SUDO_UID: asdf");
  sudoUidVar.set("-12");
  EXPECT_THROW_RE(
      info.initFromSudo(),
      std::runtime_error,
      "invalid value for SUDO_UID: -12");
  sudoUidVar.set("9999999999999999999");
  EXPECT_THROW_RE(
      info.initFromSudo(),
      std::runtime_error,
      "invalid value for SUDO_UID: 9999999999999999999");

  sudoUidVar.set("65534");
  sudoGidVar.set("");
  EXPECT_THROW_RE(
      info.initFromSudo(), std::runtime_error, "invalid value for SUDO_GID: ");
  sudoGidVar.set("hello world");
  EXPECT_THROW_RE(
      info.initFromSudo(),
      std::runtime_error,
      "invalid value for SUDO_GID: hello world");
  sudoGidVar.set("-3");
  EXPECT_THROW_RE(
      info.initFromSudo(),
      std::runtime_error,
      "invalid value for SUDO_GID: -3");
  sudoGidVar.set("19999999999999999999");
  EXPECT_THROW_RE(
      info.initFromSudo(),
      std::runtime_error,
      "invalid value for SUDO_GID: 19999999999999999999");

  // Finally, test a success case
  sudoUidVar.set("65534");
  sudoGidVar.set("65535");
  sudoUserVar.set("eden_test_user");
  setenv("HOME", "/some/path/../to/..//a/home/dir", 1);
  EXPECT_TRUE(info.initFromSudo());
  EXPECT_EQ(65534, info.getUid());
  EXPECT_EQ(65535, info.getGid());
  EXPECT_EQ("eden_test_user", info.getUsername());
  EXPECT_EQ("/some/a/home/dir", info.getHomeDirectory().stringPiece());
}

TEST(UserInfo, lookup) {
  // Call UserInfo::lookup() and try to confirm if it is doing the right thing.
  auto uid = getuid();

  // It's possible that this could throw if the test is being run by a UID that
  // doesn't actually exist in the passwd database.  Throwing in this case is
  // the correct behavior for the code, so we shouldn't really treat that as a
  // test failure if our current UID legitimately doesn't exist.  However, we
  // don't really expect the tests to be run with an unknown UID, so we don't
  // try to handle this situation for now.
  auto info = UserInfo::lookup();

  if (uid != 0) {
    // We don't bother doing much in the way of output validation in this case.
    // The initFromSudo() test above tests most of the sudo-handling logic.
  } else {
    EXPECT_EQ(uid, info.getUid());
    EXPECT_EQ(getgid(), info.getGid());
    // We don't bother testing the return value of getUsername() or
    // getHomeDirectory(), since we can't easily validate them other than just
    // repeating the same logic that UserInfo does.
    // For now this test makes sure we exercise the code path to look them up,
    // but we can't confirm their correctness.
  }
}
} // namespace eden
} // namespace facebook
