/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "scm/hg/lib/backingstore/c_api/HgNativeBackingStore.h"

#include <folly/Range.h>
#include <memory>
#include <stdexcept>
#include "scm/hg/lib/backingstore/c_api/RustBackingStore.h"

namespace facebook {
namespace eden {

HgNativeBackingStore::HgNativeBackingStore(folly::StringPiece repository) {
  RustCFallible<RustBackingStore> store(
      rust_backingstore_new(repository.data(), repository.size()),
      rust_backingstore_free);

  if (store.isError()) {
    throw std::runtime_error(store.getError());
  }

  store_ = store.unwrap();
}
} // namespace eden
} // namespace facebook
