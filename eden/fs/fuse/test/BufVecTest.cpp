/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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
