/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/scm/lib/backingstore/c_api/SaplingNativeBackingStore.h"

#include <folly/Range.h>
#include <folly/String.h>
#include <folly/io/IOBuf.h>
#include <folly/logging/xlog.h>
#include <cstddef>
#include <memory>
#include <stdexcept>

namespace sapling {

namespace {
/**
 * Convert a `CBytes` into `folly::IOBuf` without copying the underlying
 * data.
 */
std::unique_ptr<folly::IOBuf> bytesToIOBuf(CBytes* bytes) {
  return folly::IOBuf::takeOwnership(
      reinterpret_cast<void*>(bytes->ptr),
      bytes->len,
      [](void* /* buf */, void* userData) {
        sapling_cbytes_free(reinterpret_cast<CBytes*>(userData));
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
    BackingStore* store,
    Request* request,
    uintptr_t size,
    bool local,
    Fn&& fn) {
  sapling_backingstore_get_blob_batch(
      store,
      request,
      size,
      local,
      // We need to take address of the function, not to forward it.
      // @lint-ignore CLANGTIDY
      &fn,
      [](void* fn, size_t index, CFallibleBase result) {
        (*static_cast<Fn*>(fn))(index, result);
      });
}

/**
 * A helper function to make it easier to work with FFI function pointers. Only
 * non-capturing lambdas can be used as FFI function pointers. To bypass this
 * restriction, we pass in the pointer to the capturing function opaquely.
 * Whenever we get called to process the result, we call that capturing
 * function instead.
 */
template <typename Fn>
void getBlobMetadataBatchCallback(
    BackingStore* store,
    Request* request,
    uintptr_t size,
    bool local,
    Fn&& fn) {
  sapling_backingstore_get_file_aux_batch(
      store,
      request,
      size,
      local,
      // We need to take address of the function, not to forward it.
      // @lint-ignore CLANGTIDY
      &fn,
      [](void* fn, size_t index, CFallibleBase result) {
        (*static_cast<Fn*>(fn))(index, result);
      });
}

/**
 * A helper function to make it easier to work with FFI function pointers. Only
 * non-capturing lambdas can be used as FFI function pointers. To bypass this
 * restriction, we pass in the pointer to the capturing function opaquely.
 * Whenever we get called to process the result, we call that capturing
 * function instead.
 */
template <typename Fn>
void getTreeBatchCallback(
    BackingStore* store,
    Request* request,
    uintptr_t size,
    bool local,
    Fn&& fn) {
  sapling_backingstore_get_tree_batch(
      store,
      request,
      size,
      local,
      // We need to take address of the function, not to forward it.
      // @lint-ignore CLANGTIDY
      &fn,
      [](void* fn, size_t index, CFallibleBase result) {
        (*static_cast<Fn*>(fn))(index, result);
      });
}
} // namespace

SaplingNativeBackingStore::SaplingNativeBackingStore(
    std::string_view repository,
    const BackingStoreOptions& options) {
  CFallible<BackingStore, sapling_backingstore_free> store{
      sapling_backingstore_new(repository, &options)};

  if (store.isError()) {
    throw std::runtime_error(store.getError());
  }

  store_ = store.unwrap();
}

std::shared_ptr<Tree> SaplingNativeBackingStore::getTree(
    folly::ByteRange node,
    bool local) {
  XLOG(DBG7) << "Importing tree node=" << folly::hexlify(node)
             << " from hgcache";

  CFallible<Tree, sapling_tree_free> manifest{
      sapling_backingstore_get_tree(store_.get(), node, local)};

  if (manifest.isError()) {
    XLOG(DBG5) << "Error while getting tree node=" << folly::hexlify(node)
               << " from backingstore: " << manifest.getError();
    return nullptr;
  }

  return manifest.unwrap();
}

void SaplingNativeBackingStore::getTreeBatch(
    const std::vector<std::pair<folly::ByteRange, folly::ByteRange>>& requests,
    bool local,
    std::function<void(size_t, std::shared_ptr<Tree>)>&& resolve) {
  size_t count = requests.size();

  XLOG(DBG7) << "Import batch of trees with size:" << count;

  std::vector<Request> raw_requests;
  raw_requests.reserve(count);

  for (auto& [name, node] : requests) {
    raw_requests.push_back(Request{
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

  getTreeBatchCallback(
      store_.get(),
      raw_requests.data(),
      count,
      local,
      [resolve, requests, count](size_t index, CFallibleBase raw_result) {
        CFallible<Tree, sapling_tree_free> result{std::move(raw_result)};

        if (result.isError()) {
          // TODO: It would be nice if we can differentiate not found error with
          // other errors.
          auto error = result.getError();
          XLOGF(
              DBG6,
              "Failed to import path=\"{}\" node={} from EdenAPI (batch tree {}/{}): {}",
              folly::StringPiece{requests[index].first},
              folly::hexlify(requests[index].second),
              index,
              count,
              error);
        } else {
          XLOGF(
              DBG6,
              "Imported path=\"{}\" node={} from EdenAPI (batch tree: {}/{})",
              folly::StringPiece{requests[index].first},
              folly::hexlify(requests[index].second),
              index,
              count);
          resolve(index, result.unwrap());
        }
      });
}

std::unique_ptr<folly::IOBuf> SaplingNativeBackingStore::getBlob(
    folly::ByteRange name,
    folly::ByteRange node,
    bool local) {
  XLOG(DBG7) << "Importing blob name=" << name.data()
             << " node=" << folly::hexlify(node) << " from hgcache";
  CFallible<CBytes, sapling_cbytes_free> result{
      sapling_backingstore_get_blob(store_.get(), name, node, local)};

  if (result.isError()) {
    XLOG(DBG5) << "Error while getting blob name=" << name.data()
               << " node=" << folly::hexlify(node)
               << " from backingstore: " << result.getError();
    return nullptr;
  }

  return bytesToIOBuf(result.unwrap().release());
}

void SaplingNativeBackingStore::getBlobBatch(
    const std::vector<std::pair<folly::ByteRange, folly::ByteRange>>& requests,
    bool local,
    std::function<void(size_t, std::unique_ptr<folly::IOBuf>)>&& resolve) {
  size_t count = requests.size();

  XLOG(DBG7) << "Import blobs with size:" << count;

  std::vector<Request> raw_requests;
  raw_requests.reserve(count);

  for (auto& [name, node] : requests) {
    raw_requests.push_back(Request{
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
      [resolve, requests, count](size_t index, CFallibleBase raw_result) {
        CFallible<CBytes, sapling_cbytes_free> result{std::move(raw_result)};

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

std::shared_ptr<FileAuxData> SaplingNativeBackingStore::getBlobMetadata(
    folly::ByteRange node,
    bool local) {
  XLOG(DBG7) << "Importing blob metadata"
             << " node=" << folly::hexlify(node) << " from hgcache";
  CFallible<FileAuxData, sapling_file_aux_free> result{
      sapling_backingstore_get_file_aux(store_.get(), node, local)};

  if (result.isError()) {
    XLOG(DBG5) << "Error while getting blob metadata"
               << " node=" << folly::hexlify(node)
               << " from backingstore: " << result.getError();
    return nullptr;
  }

  return result.unwrap();
}

void SaplingNativeBackingStore::getBlobMetadataBatch(
    const std::vector<std::pair<folly::ByteRange, folly::ByteRange>>& requests,
    bool local,
    std::function<void(size_t, std::shared_ptr<FileAuxData>)>&& resolve) {
  size_t count = requests.size();

  XLOG(DBG7) << "Import blob metadatas with size:" << count;

  std::vector<Request> raw_requests;
  raw_requests.reserve(count);

  for (auto& [name, node] : requests) {
    raw_requests.push_back(Request{
        name.data(),
        name.size(),
        node.data(),
    });

    XLOGF(
        DBG9,
        "Processing metadata path=\"{}\" ({}) node={} ({:p})",
        name.data(),
        name.size(),
        folly::hexlify(node),
        node.data());
  }

  getBlobMetadataBatchCallback(
      store_.get(),
      raw_requests.data(),
      count,
      local,
      [resolve, requests, count](size_t index, CFallibleBase raw_result) {
        CFallible<FileAuxData, sapling_file_aux_free> result{
            std::move(raw_result)};

        if (result.isError()) {
          // TODO: It would be nice if we can differentiate not found error with
          // other errors.
          auto error = result.getError();
          XLOGF(
              DBG6,
              "Failed to import metadata path=\"{}\" node={} from EdenAPI (batch {}/{}): {}",
              folly::StringPiece{requests[index].first},
              folly::hexlify(requests[index].second),
              index,
              count,
              error);
        } else {
          auto metadata = result.unwrap();
          XLOGF(
              DBG6,
              "Imported metadata path=\"{}\" node={} from EdenAPI (batch: {}/{})",
              folly::StringPiece{requests[index].first},
              folly::hexlify(requests[index].second),
              index,
              count);
          resolve(index, std::move(metadata));
        }
      });
}

void SaplingNativeBackingStore::flush() {
  XLOG(DBG7) << "Flushing backing store";

  sapling_backingstore_flush(store_.get());
}

} // namespace sapling
