/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/LinuxKernelVersion.h"

#include <stdexcept>

#include <gtest/gtest.h>

using namespace facebook::eden;

TEST(LinuxKernelVersion, parseLinuxKernelVersion) {
  EXPECT_EQ(6, parseLinuxKernelVersion("6.13.2-0_fbk7").major);
  EXPECT_EQ(13, parseLinuxKernelVersion("6.13.2-0_fbk7").minor);
  EXPECT_EQ(5, parseLinuxKernelVersion("5.8").major);
  EXPECT_EQ(8, parseLinuxKernelVersion("5.8").minor);
  EXPECT_EQ(5, parseLinuxKernelVersion("5.8-0_fbk1").major);
  EXPECT_EQ(8, parseLinuxKernelVersion("5.8-0_fbk1").minor);
}

TEST(LinuxKernelVersion, parseLinuxKernelVersionRejectsInvalidRelease) {
  EXPECT_THROW(parseLinuxKernelVersion(""), std::invalid_argument);
  EXPECT_THROW(parseLinuxKernelVersion("kernel-6.13"), std::invalid_argument);
  EXPECT_THROW(parseLinuxKernelVersion("6"), std::invalid_argument);
  EXPECT_THROW(parseLinuxKernelVersion("6."), std::invalid_argument);
  EXPECT_THROW(parseLinuxKernelVersion("6.x"), std::invalid_argument);
}
