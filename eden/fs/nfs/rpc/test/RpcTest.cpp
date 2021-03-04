/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/nfs/rpc/Rpc.h"
#include <gtest/gtest.h>
#include "eden/fs/nfs/testharness/XdrTestUtils.h"

namespace facebook::eden {

TEST(RpcTest, enums) {
  roundtrip(auth_flavor::AUTH_NONE, sizeof(int32_t));
  roundtrip(opaque_auth{}, 2 * sizeof(uint32_t));

  roundtrip(
      rejected_reply{{reject_stat::RPC_MISMATCH, mismatch_info{0, 1}}},
      sizeof(mismatch_info) + sizeof(uint32_t));
  roundtrip(
      rejected_reply{{reject_stat::AUTH_ERROR, auth_stat::AUTH_FAILED}},
      sizeof(auth_stat) + sizeof(uint32_t));
}

} // namespace facebook::eden

#endif
