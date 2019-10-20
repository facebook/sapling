/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include <folly/Range.h>
#include <memory>
#include <optional>

#include "scm/hg/lib/backingstore/c_api/RustBackingStore.h"

namespace folly {
class IOBuf;
} // namespace folly

namespace facebook {
namespace eden {
class HgNativeBackingStore {
 public:
  explicit HgNativeBackingStore(folly::StringPiece repository);

  std::optional<folly::IOBuf> getBlob(
      folly::ByteRange name,
      folly::ByteRange node);

 private:
  std::unique_ptr<RustBackingStore, std::function<void(RustBackingStore*)>>
      store_;
};
} // namespace eden
} // namespace facebook
