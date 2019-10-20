/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "HgNativeBackingStore.h"

#include <folly/Optional.h>
#include <folly/Range.h>
#include <folly/io/IOBuf.h>
#include <folly/logging/xlog.h>
#include <memory>
#include <stdexcept>

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

folly::Optional<folly::IOBuf> HgNativeBackingStore::getBlob(
    folly::ByteRange name,
    folly::ByteRange node) {
  RustCFallible<RustCBytes> result(
      rust_backingstore_get_blob(
          store_.get(), name.data(), name.size(), node.data(), node.size()),
      rust_cbytes_free);

  if (result.isError()) {
    XLOG(ERR) << "Error while getting blob name=" << name.data()
              << " node=" << node.data()
              << "from backingstore: " << result.getError();
    return folly::none;
  }
  auto buffer = result.get();
  auto iobuf =
      folly::IOBuf{folly::IOBuf::COPY_BUFFER, buffer->ptr, buffer->len};

  return iobuf;
}
} // namespace eden
} // namespace facebook
