/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/futures/Future.h>
#include <memory>
#include "rust/cxx.h"

namespace facebook::eden {

struct SparseProfileRoot;

class RootPromise {
 public:
  explicit RootPromise(folly::Promise<rust::Box<SparseProfileRoot>> root)
      : promise(std::move(root)) {}

  folly::Promise<rust::Box<SparseProfileRoot>> promise;
};

void set_root_promise_result(
    std::shared_ptr<RootPromise> promise,
    rust::Box<::facebook::eden::SparseProfileRoot>);

void set_root_promise_error(
    std::shared_ptr<RootPromise> promise,
    rust::String error);

} // namespace facebook::eden
