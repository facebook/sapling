/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/init/Init.h>
#include <folly/portability/GFlags.h>
#include <folly/portability/GTest.h>

/*
 * This is the recommended main function for all tests.
 * The Makefile links it into all of the test programs so that tests do not need
 * to - and indeed should typically not - define their own main() functions
 */
FOLLY_ATTR_WEAK int main(int argc, char** argv);

int main(int argc, char** argv) {
  // Enable glog logging to stderr by default.
  gflags::SetCommandLineOptionWithMode(
      "logtostderr", "1", gflags::SET_FLAGS_DEFAULT);
  ::testing::InitGoogleTest(&argc, argv);
  folly::init(&argc, &argv);
  return RUN_ALL_TESTS();
}
