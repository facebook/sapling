/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/scm/lib/backingstore/include/SaplingNativeBackingStore.h"

#include <folly/Range.h>
#include <folly/String.h>
#include <folly/io/IOBuf.h>
#include <folly/logging/xlog.h>
#include <rust/cxx.h>
#include <cstddef>
#include <cstdint>
#include <memory>
#include <optional>
#include <stdexcept>
#include <type_traits>

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

} // namespace

SaplingNativeBackingStore::SaplingNativeBackingStore(
    std::string_view repository,
    const BackingStoreOptions& options)
    : store_{
          sapling_backingstore_new(
              rust::Slice<const char>{repository.data(), repository.size()},
              options)
              .into_raw(),
          [](BackingStore* backingStore) {
            auto box = rust::Box<BackingStore>::from_raw(backingStore);
          }} {}

std::optional<ManifestId> SaplingNativeBackingStore::getManifestNode(
    NodeId node) {
  XLOG(DBG7) << "Importing manifest node=" << folly::hexlify(node)
             << " from backingstore";
  try {
    static_assert(std::is_same_v<
                  ManifestId,
                  decltype(sapling_backingstore_get_manifest(
                      *store_.get(),
                      rust::Slice<const uint8_t>{node.data(), node.size()}))>);

    return sapling_backingstore_get_manifest(
        *store_.get(), rust::Slice<const uint8_t>{node.data(), node.size()});
  } catch (const rust::Error& error) {
    XLOG(DBG2) << "Error while getting manifest node=" << folly::hexlify(node)
               << " from backingstore: " << error.what();
    return std::nullopt;
  }
}

std::shared_ptr<Tree> SaplingNativeBackingStore::getTree(
    NodeId node,
    bool local) {
  XLOG(DBG7) << "Importing tree node=" << folly::hexlify(node)
             << " from hgcache";

  try {
    return sapling_backingstore_get_tree(
        *store_.get(),
        rust::Slice<const uint8_t>{node.data(), node.size()},
        local);
  } catch (const rust::Error& error) {
    XLOG(DBG5) << "Error while getting tree node=" << folly::hexlify(node)
               << " from backingstore: " << error.what();
    return nullptr;
  }
}

void SaplingNativeBackingStore::getTreeBatch(
    NodeIdRange requests,
    bool local,
    folly::FunctionRef<void(size_t, folly::Try<std::shared_ptr<Tree>>)>
        resolve) {
  size_t count = requests.size();

  XLOG(DBG7) << "Import batch of trees with size:" << count;

  std::vector<Request> raw_requests;
  raw_requests.reserve(count);
  for (auto& node : requests) {
    raw_requests.push_back(Request{
        node.data(),
    });
  }

  using ResolveResult = folly::Try<std::shared_ptr<Tree>>;

  auto inner_resolve = [&](size_t index, CFallibleBase raw_result) {
    resolve(
        index, folly::makeTryWith([&] {
          CFallible<Tree, sapling_tree_free> result{std::move(raw_result)};
          if (result.isError()) {
            XLOGF(
                DBG6,
                "Failed to import node={} from EdenAPI (batch tree {}/{}): {}",
                folly::hexlify(requests[index]),
                index,
                count,
                result.getError());
            return ResolveResult{SaplingFetchError{result.getError()}};
          } else {
            XLOGF(
                DBG6,
                "Imported node={} from EdenAPI (batch tree: {}/{})",
                folly::hexlify(requests[index]),
                index,
                count);
            return ResolveResult{std::shared_ptr<Tree>{result.unwrap()}};
          }
        }));
  };

  sapling_backingstore_get_tree_batch(
      store_.get(),
      folly::crange(raw_requests),
      local,
      &inner_resolve,
      +[](void* fn, size_t index, CFallibleBase result) {
        (*static_cast<decltype(inner_resolve)*>(fn))(index, result);
      });
}

std::unique_ptr<folly::IOBuf> SaplingNativeBackingStore::getBlob(
    NodeId node,
    bool local) {
  XLOG(DBG7) << "Importing blob node=" << folly::hexlify(node)
             << " from hgcache";

  try {
    auto blob = sapling_backingstore_get_blob(
                    *store_.get(),
                    rust::Slice<const uint8_t>{node.data(), node.size()},
                    local)
                    .into_raw();
    return folly::IOBuf::takeOwnership(
        reinterpret_cast<void*>(blob->bytes.data()),
        blob->bytes.size(),
        [](void* /* buf */, void* blob) mutable {
          auto vec = rust::Box<Blob>::from_raw(reinterpret_cast<Blob*>(blob));
        },
        reinterpret_cast<void*>(blob));
  } catch (const rust::Error& error) {
    XLOG(DBG5) << "Error while getting blob node=" << folly::hexlify(node)
               << " from backingstore: " << error.what();
    return nullptr;
  }
}

void SaplingNativeBackingStore::getBlobBatch(
    NodeIdRange requests,
    bool local,
    folly::FunctionRef<void(size_t, folly::Try<std::unique_ptr<folly::IOBuf>>)>
        resolve) {
  size_t count = requests.size();

  XLOG(DBG7) << "Import blobs with size:" << count;

  std::vector<Request> raw_requests;
  raw_requests.reserve(count);
  for (auto& node : requests) {
    raw_requests.push_back(Request{
        node.data(),
    });
  }

  using ResolveResult = folly::Try<std::unique_ptr<folly::IOBuf>>;

  auto inner_resolve = [&](size_t index, CFallibleBase raw_result) {
    resolve(
        index, folly::makeTryWith([&] {
          CFallible<CBytes, sapling_cbytes_free> result{std::move(raw_result)};

          if (result.isError()) {
            XLOGF(
                DBG6,
                "Failed to import node={} from EdenAPI (batch {}/{}): {}",
                folly::hexlify(requests[index]),
                index,
                count,
                result.getError());
            return ResolveResult{SaplingFetchError{result.getError()}};
          } else {
            auto content = bytesToIOBuf(result.unwrap().release());
            XLOGF(
                DBG6,
                "Imported node={} from EdenAPI (batch: {}/{})",
                folly::hexlify(requests[index]),
                index,
                count);
            return ResolveResult{std::move(content)};
          }
        }));
  };

  sapling_backingstore_get_blob_batch(
      store_.get(),
      folly::crange(raw_requests),
      local,
      &inner_resolve,
      +[](void* fn, size_t index, CFallibleBase result) {
        (*static_cast<decltype(inner_resolve)*>(fn))(index, result);
      });
}

std::shared_ptr<FileAuxData> SaplingNativeBackingStore::getBlobMetadata(
    NodeId node,
    bool local) {
  XLOG(DBG7) << "Importing blob metadata"
             << " node=" << folly::hexlify(node) << " from hgcache";

  try {
    return sapling_backingstore_get_file_aux(
        *store_.get(),
        rust::Slice<const uint8_t>{node.data(), node.size()},
        local);
  } catch (const rust::Error& error) {
    XLOG(DBG5) << "Error while getting blob metadata"
               << " node=" << folly::hexlify(node)
               << " from backingstore: " << error.what();
    return nullptr;
  }
}

void SaplingNativeBackingStore::getBlobMetadataBatch(
    NodeIdRange requests,
    bool local,
    folly::FunctionRef<void(size_t, folly::Try<std::shared_ptr<FileAuxData>>)>
        resolve) {
  size_t count = requests.size();

  XLOG(DBG7) << "Import blob metadatas with size:" << count;

  std::vector<Request> raw_requests;
  raw_requests.reserve(count);
  for (auto& node : requests) {
    raw_requests.push_back(Request{
        node.data(),
    });
  }

  using ResolveResult = folly::Try<std::shared_ptr<FileAuxData>>;

  auto inner_resolve = [&](size_t index, CFallibleBase raw_result) {
    resolve(
        index, folly::makeTryWith([&] {
          CFallible<FileAuxData, sapling_file_aux_free> result{
              std::move(raw_result)};

          if (result.isError()) {
            XLOGF(
                DBG6,
                "Failed to import metadata node={} from EdenAPI (batch {}/{}): {}",
                folly::hexlify(requests[index]),
                index,
                count,
                result.getError());
            return ResolveResult{SaplingFetchError{result.getError()}};
          } else {
            XLOGF(
                DBG6,
                "Imported metadata node={} from EdenAPI (batch: {}/{})",
                folly::hexlify(requests[index]),
                index,
                count);
            return ResolveResult{std::shared_ptr<FileAuxData>{result.unwrap()}};
          }
        }));
  };

  sapling_backingstore_get_file_aux_batch(
      store_.get(),
      folly::crange(raw_requests),
      local,
      &inner_resolve,
      +[](void* fn, size_t index, CFallibleBase result) {
        (*static_cast<decltype(inner_resolve)*>(fn))(index, result);
      });
}

void SaplingNativeBackingStore::flush() {
  XLOG(DBG7) << "Flushing backing store";

  sapling_backingstore_flush(*store_.get());
}

} // namespace sapling
