/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include <utility>

#include <folly/portability/GTest.h>

#include "eden/fs/nfs/NfsUtils.h"
#include "eden/fs/nfs/NfsdRpc.h"

namespace facebook::eden {

TEST(AccessTest, read) {
  struct stat st {};
  st.st_mode = S_IRUSR;
  EXPECT_EQ(getEffectiveAccessRights(st, ACCESS3_READ), ACCESS3_READ);
  EXPECT_EQ(getEffectiveAccessRights(st, ACCESS3_LOOKUP), ACCESS3_LOOKUP);

  st.st_mode = S_IRGRP;
  EXPECT_EQ(getEffectiveAccessRights(st, ACCESS3_READ), ACCESS3_READ);
  EXPECT_EQ(getEffectiveAccessRights(st, ACCESS3_LOOKUP), ACCESS3_LOOKUP);

  st.st_mode = S_IROTH;
  EXPECT_EQ(getEffectiveAccessRights(st, ACCESS3_READ), ACCESS3_READ);
  EXPECT_EQ(getEffectiveAccessRights(st, ACCESS3_LOOKUP), ACCESS3_LOOKUP);

  st.st_mode = S_IRGRP | S_IROTH;
  EXPECT_EQ(getEffectiveAccessRights(st, ACCESS3_READ), ACCESS3_READ);
  EXPECT_EQ(getEffectiveAccessRights(st, ACCESS3_LOOKUP), ACCESS3_LOOKUP);

  st.st_mode = S_IWGRP | S_IXOTH;
  EXPECT_EQ(getEffectiveAccessRights(st, ACCESS3_READ), 0);
  EXPECT_EQ(getEffectiveAccessRights(st, ACCESS3_LOOKUP), 0);
}

TEST(AccessTest, write) {
  struct stat st {};
  st.st_mode = S_IWUSR;
  EXPECT_EQ(getEffectiveAccessRights(st, ACCESS3_MODIFY), ACCESS3_MODIFY);
  EXPECT_EQ(getEffectiveAccessRights(st, ACCESS3_EXTEND), ACCESS3_EXTEND);
  EXPECT_EQ(getEffectiveAccessRights(st, ACCESS3_DELETE), 0);

  st.st_mode = S_IWGRP;
  EXPECT_EQ(getEffectiveAccessRights(st, ACCESS3_MODIFY), ACCESS3_MODIFY);
  EXPECT_EQ(getEffectiveAccessRights(st, ACCESS3_EXTEND), ACCESS3_EXTEND);
  EXPECT_EQ(getEffectiveAccessRights(st, ACCESS3_DELETE), 0);

  st.st_mode = S_IWOTH;
  EXPECT_EQ(getEffectiveAccessRights(st, ACCESS3_MODIFY), ACCESS3_MODIFY);
  EXPECT_EQ(getEffectiveAccessRights(st, ACCESS3_EXTEND), ACCESS3_EXTEND);
  EXPECT_EQ(getEffectiveAccessRights(st, ACCESS3_DELETE), 0);

  st.st_mode = S_IWGRP | S_IWOTH;
  EXPECT_EQ(getEffectiveAccessRights(st, ACCESS3_MODIFY), ACCESS3_MODIFY);
  EXPECT_EQ(getEffectiveAccessRights(st, ACCESS3_EXTEND), ACCESS3_EXTEND);
  EXPECT_EQ(getEffectiveAccessRights(st, ACCESS3_DELETE), 0);

  st.st_mode = S_IRUSR | S_IRGRP;
  EXPECT_EQ(getEffectiveAccessRights(st, ACCESS3_MODIFY), 0);
  EXPECT_EQ(getEffectiveAccessRights(st, ACCESS3_EXTEND), 0);
  EXPECT_EQ(getEffectiveAccessRights(st, ACCESS3_DELETE), 0);

  st.st_mode = S_IWGRP | S_IWOTH | S_IFDIR;
  EXPECT_EQ(getEffectiveAccessRights(st, ACCESS3_MODIFY), ACCESS3_MODIFY);
  EXPECT_EQ(getEffectiveAccessRights(st, ACCESS3_EXTEND), ACCESS3_EXTEND);
  EXPECT_EQ(getEffectiveAccessRights(st, ACCESS3_DELETE), ACCESS3_DELETE);

  st.st_mode = S_IRUSR | S_IRGRP | S_IFDIR;
  EXPECT_EQ(getEffectiveAccessRights(st, ACCESS3_MODIFY), 0);
  EXPECT_EQ(getEffectiveAccessRights(st, ACCESS3_EXTEND), 0);
  EXPECT_EQ(getEffectiveAccessRights(st, ACCESS3_DELETE), 0);
}

TEST(AccessTest, execute) {
  struct stat st {};
  st.st_mode = S_IXUSR;
  EXPECT_EQ(getEffectiveAccessRights(st, ACCESS3_EXECUTE), ACCESS3_EXECUTE);

  st.st_mode = S_IXGRP;
  EXPECT_EQ(getEffectiveAccessRights(st, ACCESS3_EXECUTE), ACCESS3_EXECUTE);

  st.st_mode = S_IXOTH;
  EXPECT_EQ(getEffectiveAccessRights(st, ACCESS3_EXECUTE), ACCESS3_EXECUTE);

  st.st_mode = S_IXGRP | S_IXOTH;
  EXPECT_EQ(getEffectiveAccessRights(st, ACCESS3_EXECUTE), ACCESS3_EXECUTE);

  st.st_mode = S_IRUSR | S_IWUSR;
  EXPECT_EQ(getEffectiveAccessRights(st, ACCESS3_EXECUTE), 0);
}

} // namespace facebook::eden

#endif
