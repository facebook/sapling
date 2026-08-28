/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/config/DeadConfigKeys.h"

#include <gtest/gtest.h>

namespace facebook::eden {

TEST(DeadConfigKeysTest, matchesExactSectionAndKey) {
  EXPECT_TRUE(isDeadConfigKey("experimental", "prefetch-optimizations-v2"));
  EXPECT_TRUE(isDeadConfigKey("coroutines", "enable-phase1"));
}

TEST(DeadConfigKeysTest, doesNotMatchPartialOrShiftedKeys) {
  EXPECT_FALSE(isDeadConfigKey("experimental", "prefetch-optimizations"));
  EXPECT_FALSE(isDeadConfigKey("coroutines", "enable-phase"));
  EXPECT_FALSE(isDeadConfigKey("core", "edenDirectory"));
  // The section/key split must line up with the ':' in the dead key.
  EXPECT_FALSE(isDeadConfigKey("coroutines:enable", "phase1"));
}

} // namespace facebook::eden
