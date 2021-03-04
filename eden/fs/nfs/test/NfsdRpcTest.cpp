/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/nfs/NfsdRpc.h"
#include <gtest/gtest.h>
#include "eden/fs/nfs/testharness/XdrTestUtils.h"

namespace facebook::eden {

struct ResOk {
  int a;
};
EDEN_XDR_SERDE_DECL(ResOk, a);
EDEN_XDR_SERDE_IMPL(ResOk, a);

struct ResFail {
  int b;
};
EDEN_XDR_SERDE_DECL(ResFail, b);
EDEN_XDR_SERDE_IMPL(ResFail, b);

struct FullVariant : public detail::Nfsstat3Variant<ResOk, ResFail> {};

struct EmptyFailVariant : public detail::Nfsstat3Variant<ResOk> {};

TEST(NfsdRpcTest, variant) {
  FullVariant var1{{{nfsstat3::NFS3_OK, ResOk{42}}}};
  roundtrip(var1, 2 * sizeof(uint32_t));

  FullVariant var2{{{nfsstat3::NFS3ERR_PERM, ResFail{10}}}};
  roundtrip(var2, 2 * sizeof(uint32_t));

  EmptyFailVariant var3{{{nfsstat3::NFS3_OK, ResOk{42}}}};
  roundtrip(var3, 2 * sizeof(uint32_t));

  EmptyFailVariant var4{{{nfsstat3::NFS3ERR_PERM, std::monostate{}}}};
  roundtrip(var4, sizeof(uint32_t));
}

} // namespace facebook::eden

#endif
