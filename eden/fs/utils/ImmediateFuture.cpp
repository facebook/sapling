/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/ImmediateFuture.h"

namespace facebook::eden {
ImmediateFuture<folly::Unit> makeNotReadyImmediateFuture() {
  // A promise is necessary to prevent the ImmediateFuture constructor from
  // detecting that the SemiFuture is immediately ready and unwrapping it.
  auto [promise, semi] = folly::makePromiseContract<folly::Unit>();
  auto ret = ImmediateFuture{std::move(semi)};
  promise.setValue(folly::unit);
  return ret;
}
} // namespace facebook::eden
