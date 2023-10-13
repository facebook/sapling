/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/scm/lib/edenfs-ffi/src/ffi.h"
#include <memory>
#include <utility>

namespace facebook::eden {

void set_root_promise_result(
    std::shared_ptr<RootPromise> rootPromise,
    rust::Box<SparseProfileRoot> root) {
  rootPromise->promise.setValue(std::move(root));
  return;
}

void set_root_promise_error(
    std::shared_ptr<RootPromise> rootPromise,
    rust::String error) {
  rootPromise->promise.setException(
      std::runtime_error(std::move(error).c_str()));
  return;
}
} // namespace facebook::eden
