/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include <folly/Range.h>
#include <memory>

// This is relative path because of the CMake build since it does not preserve
// the directory structure we have in the repository
#include "RustBackingStore.h" // @manual

namespace folly {
class IOBuf;
template <typename T>
class Optional;
} // namespace folly

namespace facebook {
namespace eden {
class HgNativeBackingStore {
 public:
  explicit HgNativeBackingStore(folly::StringPiece repository);

  folly::Optional<folly::IOBuf> getBlob(
      folly::ByteRange name,
      folly::ByteRange node);

 private:
  std::unique_ptr<RustBackingStore, std::function<void(RustBackingStore*)>>
      store_;
};
} // namespace eden
} // namespace facebook
