/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/scm/lib/backingstore/include/SaplingNativeBackingStore.h"
#include "eden/scm/lib/backingstore/src/ffi.rs.h" // @manual

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

SaplingNativeBackingStore::SaplingNativeBackingStore(
    std::string_view repository,
    const SaplingNativeBackingStoreOptions& options)
    : store_{
          sapling_backingstore_new(
              rust::Slice<const char>{repository.data(), repository.size()},
              options)
              .into_raw(),
          [](BackingStore* backingStore) {
            auto box = rust::Box<BackingStore>::from_raw(backingStore);
          }} {
  try {
    repoName_ = std::string(sapling_backingstore_get_name(*store_.get()));
  } catch (const rust::Error& error) {
    XLOGF(DBG2, "Error while repo name from backingstore: {}", error.what());
  }
}

std::optional<ManifestId> SaplingNativeBackingStore::getManifestNode(
    NodeId node) {
  XLOGF(
      DBG7,
      "Importing manifest node={} from backingstore",
      folly::hexlify(node));
  try {
    static_assert(std::is_same_v<
                  ManifestId,
                  decltype(sapling_backingstore_get_manifest(
                      *store_.get(),
                      rust::Slice<const uint8_t>{node.data(), node.size()}))>);

    return sapling_backingstore_get_manifest(
        *store_.get(), rust::Slice<const uint8_t>{node.data(), node.size()});
  } catch (const rust::Error& error) {
    XLOGF(
        DBG2,
        "Error while getting manifest node={} from backingstore: {}",
        folly::hexlify(node),
        error.what());
    return std::nullopt;
  }
}

folly::Try<std::shared_ptr<Tree>> SaplingNativeBackingStore::getTree(
    NodeId node,
    FetchMode fetch_mode) {
  XLOGF(DBG7, "Importing tree node={} from hgcache", folly::hexlify(node));
  return folly::makeTryWith([&] {
    auto tree = sapling_backingstore_get_tree(
        *store_.get(),
        rust::Slice<const uint8_t>{node.data(), node.size()},
        fetch_mode);
    XCHECK(
        tree.get(),
        "sapling_backingstore_get_tree returned a nullptr, but did not throw an exception.");
    return tree;
  });
}

void SaplingNativeBackingStore::getTreeBatch(
    SaplingRequestRange requests,
    sapling::FetchMode fetch_mode,
    folly::FunctionRef<void(size_t, folly::Try<std::shared_ptr<Tree>>)>
        resolve) {
  auto resolver = std::make_shared<GetTreeBatchResolver>(std::move(resolve));
  auto count = requests.size();

  XLOGF(DBG7, "Import batch of trees with size:{}", count);

  std::vector<Request> raw_requests;
  raw_requests.reserve(count);
  for (auto& request : requests) {
    raw_requests.push_back(Request{
        request.node.data(),
        request.cause,
    });
  }

  sapling_backingstore_get_tree_batch(
      *store_.get(),
      rust::Slice<const Request>{raw_requests.data(), raw_requests.size()},
      fetch_mode,
      std::move(resolver));
}

folly::Try<std::shared_ptr<TreeAuxData>>
SaplingNativeBackingStore::getTreeMetadata(NodeId node, bool local) {
  FetchMode fetch_mode = FetchMode::AllowRemote;
  if (local) {
    fetch_mode = FetchMode::LocalOnly;
  }
  XLOGF(
      DBG7,
      "Importing tree metadata node={} from hgcache",
      folly::hexlify(node));
  return folly::makeTryWith([&] {
    auto metadata = sapling_backingstore_get_tree_aux(
        *store_.get(),
        rust::Slice<const uint8_t>{node.data(), node.size()},
        fetch_mode);
    XCHECK(
        metadata.get(),
        "sapling_backingstore_get_tree_aux returned a nullptr, but did not throw an exception.");
    return metadata;
  });
}

void SaplingNativeBackingStore::getTreeMetadataBatch(
    SaplingRequestRange requests,
    sapling::FetchMode fetch_mode,
    folly::FunctionRef<void(size_t, folly::Try<std::shared_ptr<TreeAuxData>>)>
        resolve) {
  auto resolver = std::make_shared<GetTreeAuxBatchResolver>(std::move(resolve));
  auto count = requests.size();

  XLOGF(DBG7, "Import tree metadatas with size: {}", count);

  std::vector<Request> raw_requests;
  raw_requests.reserve(count);
  for (auto& request : requests) {
    raw_requests.push_back(Request{
        request.node.data(),
        request.cause,
    });
  }

  sapling_backingstore_get_tree_aux_batch(
      *store_.get(),
      rust::Slice<const Request>{raw_requests.data(), raw_requests.size()},
      fetch_mode,
      std::move(resolver));
}

folly::Try<std::unique_ptr<folly::IOBuf>> SaplingNativeBackingStore::getBlob(
    NodeId node,
    FetchMode fetch_mode) {
  XLOGF(DBG7, "Importing blob node={} from hgcache", folly::hexlify(node));
  return folly::makeTryWith([&] {
    auto blob = sapling_backingstore_get_blob(
                    *store_.get(),
                    rust::Slice<const uint8_t>{node.data(), node.size()},
                    fetch_mode)
                    .into_raw();
    XCHECK(
        blob,
        "sapling_backingstore_get_blob returned a nullptr, but did not throw an exception.");
    return folly::IOBuf::takeOwnership(
        reinterpret_cast<void*>(blob->bytes.data()),
        blob->bytes.size(),
        [](void* /* buf */, void* blob) mutable {
          auto vec = rust::Box<Blob>::from_raw(reinterpret_cast<Blob*>(blob));
        },
        reinterpret_cast<void*>(blob));
  });
}

void SaplingNativeBackingStore::getBlobBatch(
    SaplingRequestRange requests,
    sapling::FetchMode fetch_mode,
    folly::FunctionRef<void(size_t, folly::Try<std::unique_ptr<folly::IOBuf>>)>
        resolve) {
  auto resolver = std::make_shared<GetBlobBatchResolver>(std::move(resolve));
  auto count = requests.size();

  XLOGF(DBG7, "Import blobs with size: {}", count);

  std::vector<Request> raw_requests;
  raw_requests.reserve(count);
  for (auto& request : requests) {
    raw_requests.push_back(Request{
        request.node.data(),
        request.cause,
    });
  }

  sapling_backingstore_get_blob_batch(
      *store_.get(),
      rust::Slice<const Request>{raw_requests.data(), raw_requests.size()},
      fetch_mode,
      std::move(resolver));
}

folly::Try<std::shared_ptr<FileAuxData>>
SaplingNativeBackingStore::getBlobMetadata(NodeId node, bool local) {
  FetchMode fetch_mode = FetchMode::AllowRemote;
  if (local) {
    fetch_mode = FetchMode::LocalOnly;
  }
  XLOGF(
      DBG7,
      "Importing blob metadata node={} from hgcache",
      folly::hexlify(node));
  return folly::makeTryWith([&] {
    auto metadata = sapling_backingstore_get_file_aux(
        *store_.get(),
        rust::Slice<const uint8_t>{node.data(), node.size()},
        fetch_mode);
    XCHECK(
        metadata.get(),
        "sapling_backingstore_get_file_aux returned a nullptr, but did not throw an exception.");
    return metadata;
  });
}

void SaplingNativeBackingStore::getBlobMetadataBatch(
    SaplingRequestRange requests,
    sapling::FetchMode fetch_mode,
    folly::FunctionRef<void(size_t, folly::Try<std::shared_ptr<FileAuxData>>)>
        resolve) {
  auto resolver = std::make_shared<GetFileAuxBatchResolver>(std::move(resolve));
  auto count = requests.size();

  XLOGF(DBG7, "Import blob metadatas with size: {}", count);

  std::vector<Request> raw_requests;
  raw_requests.reserve(count);
  for (auto& request : requests) {
    raw_requests.push_back(Request{
        request.node.data(),
        request.cause,
    });
  }

  sapling_backingstore_get_file_aux_batch(
      *store_.get(),
      rust::Slice<const Request>{raw_requests.data(), raw_requests.size()},
      fetch_mode,
      std::move(resolver));
}

folly::Try<std::shared_ptr<GlobFilesResponse>>
SaplingNativeBackingStore::getGlobFiles(
    // Human Readable 40b commit id
    std::string_view commit_id,
    const std::vector<std::string>& suffixes) {
  rust::Vec<rust::String> rust_suffixes;
  std::copy(
      suffixes.begin(), suffixes.end(), std::back_inserter(rust_suffixes));

  auto br = folly::ByteRange(commit_id);
  return folly::makeTryWith([&] {
    auto globFiles = sapling_backingstore_get_glob_files(
        *store_.get(),
        rust::Slice<const uint8_t>{br.data(), br.size()},
        rust_suffixes);

    XCHECK(
        globFiles.get(),
        "sapling_backingstore_get_glob_files returned a nullptr, but did not throw an exception.");
    return globFiles;
  });
}

void SaplingNativeBackingStore::flush() {
  XLOG(DBG7, "Flushing backing store");

  sapling_backingstore_flush(*store_.get());
}

} // namespace sapling
