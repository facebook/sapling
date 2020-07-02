/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/ObjectFetchContext.h"

namespace {
using namespace facebook::eden;
class NullObjectFetchContext : public ObjectFetchContext {};
} // namespace

namespace facebook {
namespace eden {

ObjectFetchContext& ObjectFetchContext::getNullContext() {
  static auto* p = new NullObjectFetchContext;
  return *p;
}

} // namespace eden
} // namespace facebook
