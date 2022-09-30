/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/nfs/DirList.h"
#include <folly/portability/GTest.h>
#include "eden/fs/nfs/NfsdRpc.h"
#include "eden/fs/nfs/testharness/XdrTestUtils.h"

namespace facebook::eden {

namespace {
size_t computeInitialOverhead() {
  return XdrTrait<post_op_attr>::serializedSize(post_op_attr{fattr3{}}) +
      XdrTrait<uint64_t>::serializedSize(0) +
      2 * XdrTrait<bool>::serializedSize(false);
}
} // namespace

TEST(DirListTest, size) {
  // Verify that the computeInitialOverhead in DirList.cpp is correct.  If this
  // fails, do not modify the constant referenced below! It means that the XDR
  // datastructures have changed and no longer have the correct size.
  EXPECT_EQ(computeInitialOverhead(), kNfsDirListInitialOverhead);
}

} // namespace facebook::eden

#endif
