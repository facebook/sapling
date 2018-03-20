/*
 *  Copyright (c) 2017-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/fuse/BufVec.h"

#include <gtest/gtest.h>

TEST(BufVecTest, BufVec) {
  auto root = folly::IOBuf::wrapBuffer("hello", 5);
  root->appendChain(folly::IOBuf::wrapBuffer("world", 5));
  const auto bufVec = facebook::eden::BufVec{std::move(root)};
  EXPECT_EQ(10u, bufVec.size());
  EXPECT_EQ(10u, bufVec.copyData().size());
  EXPECT_EQ("helloworld", bufVec.copyData());
}
