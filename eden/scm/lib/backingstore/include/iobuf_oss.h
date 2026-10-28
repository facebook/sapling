/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#pragma once

#include <folly/io/IOBuf.h>
#include <rust/cxx.h>
#include <cstdint>
#include <memory>

namespace sapling {

/** Used by src/oss.rs, see the comment there. */
inline std::unique_ptr<folly::IOBuf> sapling_backingstore_iobuf_copy_buffer(
    rust::Slice<const uint8_t> data) {
  return folly::IOBuf::copyBuffer(data.data(), data.size());
}

} // namespace sapling
