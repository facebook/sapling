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
#include <cstddef>
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

/**
 * A helper function to make it easier to work with FFI function pointers. Only
 * non-capturing lambdas can be used as FFI function pointers. To bypass this
 * restriction, we pass in the pointer to the capturing function opaquely.
 * Whenever we get called to process the result, we call that capturing
 * function instead.
 */
template <typename Fn>
void getBlobBatchCallback(
    RustBackingStore* store,
    RustRequest* request,
    uintptr_t size,
    bool local,
    Fn&& fn) {
  rust_backingstore_get_blob_batch(
      store,
      request,
      size,
      local,
      // We need to take address of the function, not to forward it.
      // @lint-ignore CLANGTIDY
      &fn,
      [](void* fn, size_t index, RustCFallibleBase result) {
        (*static_cast<Fn*>(fn))(index, result);
      });
}
} // namespace

HgNativeBackingStore::HgNativeBackingStore(
    folly::StringPiece repository,
    bool useEdenApi) {
  RustCFallible<RustBackingStore> store(
      rust_backingstore_new(repository.data(), repository.size(), useEdenApi),
      rust_backingstore_free);

  if (store.isError()) {
    throw std::runtime_error(store.getError());
  }

  store_ = store.unwrap();
}

std::unique_ptr<folly::IOBuf> HgNativeBackingStore::getBlob(
    folly::ByteRange name,
    folly::ByteRange node,
    bool local) {
  XLOG(DBG7) << "Importing blob name=" << name.data()
             << " node=" << folly::hexlify(node) << " from hgcache";
  RustCFallible<RustCBytes> result(
      rust_backingstore_get_blob(
          store_.get(),
          name.data(),
          name.size(),
          node.data(),
          node.size(),
          local),
      rust_cbytes_free);

  if (result.isError()) {
    XLOG(DBG5) << "Error while getting blob name=" << name.data()
               << " node=" << folly::hexlify(node)
               << " from backingstore: " << result.getError();
    return nullptr;
  }

  return bytesToIOBuf(result.unwrap().release());
}

void HgNativeBackingStore::getBlobBatch(
    const std::vector<std::pair<folly::ByteRange, folly::ByteRange>>& requests,
    bool local,
    std::function<void(size_t, std::unique_ptr<folly::IOBuf>)>&& resolve) {
  size_t count = requests.size();

  XLOG(DBG7) << "Import blobs with size:" << count;

  std::vector<RustRequest> raw_requests;
  raw_requests.reserve(count);

  for (auto& request : requests) {
    auto& name = request.first;
    auto& node = request.second;

    raw_requests.emplace_back(RustRequest{
        name.data(),
        name.size(),
        node.data(),
    });

    XLOGF(
        DBG9,
        "Processing path=\"{}\" ({}) node={} ({:p})",
        name.data(),
        name.size(),
        folly::hexlify(node),
        node.data());
  }

  getBlobBatchCallback(
      store_.get(),
      raw_requests.data(),
      count,
      local,
      [resolve, requests, count](size_t index, RustCFallibleBase raw_result) {
        RustCFallible<RustCBytes> result(
            std::move(raw_result), rust_cbytes_free);

        if (result.isError()) {
          // TODO: It would be nice if we can differentiate not found error with
          // other errors.
          auto error = result.getError();
          XLOGF(
              DBG6,
              "Failed to import path=\"{}\" node={} from EdenAPI (batch {}/{}): {}",
              folly::StringPiece{requests[index].first},
              folly::hexlify(requests[index].second),
              index,
              count,
              error);
        } else {
          auto content = bytesToIOBuf(result.unwrap().release());
          XLOGF(
              DBG6,
              "Imported path=\"{}\" node={} from EdenAPI (batch: {}/{})",
              folly::StringPiece{requests[index].first},
              folly::hexlify(requests[index].second),
              index,
              count);
          resolve(index, std::move(content));
        }
      });
}

std::shared_ptr<RustTree> HgNativeBackingStore::getTree(folly::ByteRange node) {
  XLOG(DBG7) << "Importing tree node=" << folly::hexlify(node)
             << " from hgcache";

  RustCFallible<RustTree> manifest(
      rust_backingstore_get_tree(store_.get(), node.data(), node.size()),
      rust_tree_free);

  if (manifest.isError()) {
    XLOG(DBG5) << "Error while getting tree node=" << folly::hexlify(node)
               << " from backingstore: " << manifest.getError();
    return nullptr;
  }

  return manifest.unwrap();
}

void HgNativeBackingStore::refresh() {
  XLOG(DBG7) << "Refreshing backing store";

  rust_backingstore_refresh(store_.get());
}
} // namespace eden
} // namespace facebook
