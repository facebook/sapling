/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/scm/lib/backingstore/c_api/HgNativeBackingStore.h"

#include <folly/Range.h>
#include <folly/String.h>
#include <folly/io/IOBuf.h>
#include <folly/logging/xlog.h>
#include <memory>
#include <stdexcept>

namespace facebook {
namespace eden {
namespace {
/**
 * Convert a `RustCBytes` into `folly::IOBuf` without copying the underlying
 * data.
 */
std::unique_ptr<folly::IOBuf> bytesToIOBuf(RustCBytes* bytes) {
  return folly::IOBuf::takeOwnership(
      reinterpret_cast<void*>(bytes->ptr),
      bytes->len,
      [](void* /* buf */, void* userData) {
        rust_cbytes_free(reinterpret_cast<RustCBytes*>(userData));
      },
      reinterpret_cast<void*>(bytes));
}
} // namespace

HgNativeBackingStore::HgNativeBackingStore(folly::StringPiece repository) {
  RustCFallible<RustBackingStore> store(
      rust_backingstore_new(repository.data(), repository.size()),
      rust_backingstore_free);

  if (store.isError()) {
    throw std::runtime_error(store.getError());
  }

  store_ = store.unwrap();
}

std::unique_ptr<folly::IOBuf> HgNativeBackingStore::getBlob(
    folly::ByteRange name,
    folly::ByteRange node) {
  RustCFallible<RustCBytes> result(
      rust_backingstore_get_blob(
          store_.get(), name.data(), name.size(), node.data(), node.size()),
      rust_cbytes_free);

  if (result.isError()) {
    XLOG(DBG5) << "Error while getting blob name=" << name.data()
               << " node=" << folly::hexlify(node)
               << " from backingstore: " << result.getError();
    return nullptr;
  }

  return bytesToIOBuf(result.unwrap().release());
}

std::shared_ptr<RustTree> HgNativeBackingStore::getTree(folly::ByteRange node) {
  RustCFallible<RustTree> manifest(
      rust_backingstore_get_tree(store_.get(), node.data(), node.size()),
      rust_tree_free);

  if (manifest.isError()) {
    XLOG(DBG5) << "Error while getting tree "
               << " node=" << folly::hexlify(node)
               << " from backingstore: " << manifest.getError();
    return nullptr;
  }

  return manifest.unwrap();
}
} // namespace eden
} // namespace facebook
