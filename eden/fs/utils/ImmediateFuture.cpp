/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/ImmediateFuture.h"

namespace facebook::eden {
ImmediateFuture<folly::Unit> makeNotReadyImmediateFuture() {
  return ImmediateFuture{
      folly::SemiFuture<folly::Unit>{folly::unit},
      ImmediateFuture<folly::Unit>::SemiFutureReadiness::LazySemiFuture};
}
} // namespace facebook::eden
