/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/nfs/NfsdRpc.h"
#include <folly/portability/GTest.h>
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
  roundtrip(var1);

  FullVariant var2{{{nfsstat3::NFS3ERR_PERM, ResFail{10}}}};
  roundtrip(var2);

  EmptyFailVariant var3{{{nfsstat3::NFS3_OK, ResOk{42}}}};
  roundtrip(var3);

  EmptyFailVariant var4{{{nfsstat3::NFS3ERR_PERM, std::monostate{}}}};
  roundtrip(var4);
}

} // namespace facebook::eden

#endif
