/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
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
    std::string_view mount)
    : store_{
          sapling_backingstore_new(
              rust::Slice<const char>{repository.data(), repository.size()},
              rust::Slice<const char>{mount.data(), mount.size()})
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

// Fetch a single tree. "Not found" is propagated as nullptr to avoid exception
// overhead.
folly::Try<std::shared_ptr<Tree>> SaplingNativeBackingStore::getTree(
    NodeId node,
    RepoPath path,
    const ObjectFetchContextPtr& context,
    FetchMode fetch_mode) {
  XLOGF(DBG7, "Importing tree node={} from hgcache", folly::hexlify(node));
  return folly::makeTryWith([&] {
    auto tree = sapling_backingstore_get_tree(
        *store_.get(),
        rust::Slice<const uint8_t>{node.data(), node.size()},
        fetch_mode);

    if (tree && context->getCause() != FetchCause::Prefetch) {
      sapling_backingstore_witness_dir_read(
          *store_.get(),
          rust::Slice<const uint8_t>{
              reinterpret_cast<const uint8_t*>(path.view().data()),
              path.view().size()},
          *tree,
          fetch_mode == FetchMode::LocalOnly,
          context->getClientPid().valueOrZero().get());
    }

    return tree;
  });
}

// Batch fetch trees. "Not found" is propagated as an exception.
void SaplingNativeBackingStore::getTreeBatch(
    SaplingRequestRange requests,
    sapling::FetchMode fetch_mode,
    folly::FunctionRef<void(size_t, folly::Try<std::shared_ptr<Tree>>)>
        resolve) {
  auto count = requests.size();
  if (count == 0) {
    return;
  }

  auto resolver = std::make_shared<GetTreeBatchResolver>(std::move(resolve));

  XLOGF(
      DBG7,
      "Import batch of trees with size: {}, first path: {}",
      count,
      requests[0].path);

  std::vector<Request> raw_requests;
  raw_requests.reserve(count);
  for (auto& request : requests) {
    raw_requests.push_back(Request{
        request.node.data(),
        request.cause,
        request.path.view().data(),
        request.path.view().size(),
        request.context->getClientPid().valueOrZero().get(),
    });
  }

  sapling_backingstore_get_tree_batch(
      *store_.get(),
      rust::Slice<const Request>{raw_requests.data(), raw_requests.size()},
      fetch_mode,
      std::move(resolver));
}

folly::Try<std::shared_ptr<TreeAuxData>>
SaplingNativeBackingStore::getTreeAuxData(NodeId node, bool local) {
  FetchMode fetch_mode = FetchMode::AllowRemote;
  if (local) {
    fetch_mode = FetchMode::LocalOnly;
  }
  XLOGF(
      DBG7,
      "Importing tree aux data node={} from hgcache",
      folly::hexlify(node));
  return folly::makeTryWith([&] {
    return sapling_backingstore_get_tree_aux(
        *store_.get(),
        rust::Slice<const uint8_t>{node.data(), node.size()},
        fetch_mode);
  });
}

void SaplingNativeBackingStore::getTreeAuxDataBatch(
    SaplingRequestRange requests,
    sapling::FetchMode fetch_mode,
    folly::FunctionRef<void(size_t, folly::Try<std::shared_ptr<TreeAuxData>>)>
        resolve) {
  auto resolver = std::make_shared<GetTreeAuxBatchResolver>(std::move(resolve));
  auto count = requests.size();

  XLOGF(DBG7, "Import tree aux data with size: {}", count);

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

// Fetch a single blob. "Not found" is propagated as nullptr to avoid exception
// overhead.
folly::Try<std::unique_ptr<folly::IOBuf>> SaplingNativeBackingStore::getBlob(
    NodeId node,
    RepoPath path,
    const ObjectFetchContextPtr& context,
    FetchMode fetch_mode) {
  XLOGF(DBG7, "Importing blob node={} from hgcache", folly::hexlify(node));
  return folly::makeTryWith([&] {
    auto blob = sapling_backingstore_get_blob(
        *store_.get(),
        rust::Slice<const uint8_t>{node.data(), node.size()},
        fetch_mode);

    if (blob && context->getCause() != FetchCause::Prefetch) {
      sapling_backingstore_witness_file_read(
          *store_.get(),
          rust::Str{path.view().data(), path.view().size()},
          fetch_mode == FetchMode::LocalOnly,
          context->getClientPid().valueOrZero().get());
    }

    return blob;
  });
}

// Batch fetch blobs. "Not found" is propagated as an exception.
void SaplingNativeBackingStore::getBlobBatch(
    SaplingRequestRange requests,
    sapling::FetchMode fetch_mode,
    bool allow_ignore_result,
    folly::FunctionRef<void(size_t, folly::Try<std::unique_ptr<folly::IOBuf>>)>
        resolve) {
  auto count = requests.size();
  if (count == 0) {
    return;
  }

  auto resolver = std::make_shared<GetBlobBatchResolver>(std::move(resolve));

  XLOGF(
      DBG7,
      "Import blobs with size: {}, first path: {}",
      count,
      requests[0].path);

  std::vector<Request> raw_requests;
  raw_requests.reserve(count);
  for (auto& request : requests) {
    raw_requests.push_back(Request{
        request.node.data(),
        request.cause,
    });

    if (request.cause != FetchCause::Prefetch) {
      sapling_backingstore_witness_file_read(
          *store_.get(),
          rust::Str{request.path.view().data(), request.path.view().size()},
          fetch_mode == FetchMode::LocalOnly,
          request.context->getClientPid().valueOrZero().get());
    }
  }

  sapling_backingstore_get_blob_batch(
      *store_.get(),
      rust::Slice<const Request>{raw_requests.data(), raw_requests.size()},
      fetch_mode,
      allow_ignore_result,
      std::move(resolver));
}

folly::Try<std::shared_ptr<FileAuxData>>
SaplingNativeBackingStore::getBlobAuxData(NodeId node, bool local) {
  FetchMode fetch_mode = FetchMode::AllowRemote;
  if (local) {
    fetch_mode = FetchMode::LocalOnly;
  }
  XLOGF(
      DBG7,
      "Importing blob aux data node={} from hgcache",
      folly::hexlify(node));
  return folly::makeTryWith([&] {
    return sapling_backingstore_get_file_aux(
        *store_.get(),
        rust::Slice<const uint8_t>{node.data(), node.size()},
        fetch_mode);
  });
}

void SaplingNativeBackingStore::getBlobAuxDataBatch(
    SaplingRequestRange requests,
    sapling::FetchMode fetch_mode,
    folly::FunctionRef<void(size_t, folly::Try<std::shared_ptr<FileAuxData>>)>
        resolve) {
  auto resolver = std::make_shared<GetFileAuxBatchResolver>(std::move(resolve));
  auto count = requests.size();

  XLOGF(DBG7, "Import blob aux data with size: {}", count);

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

bool SaplingNativeBackingStore::dogfoodingHost() const {
  return sapling_dogfooding_host(*store_.get());
}

void SaplingNativeBackingStore::workingCopyParentHint(const RootId& parent) {
  sapling_backingstore_set_parent_hint(*store_.get(), parent.value());
}

folly::Try<std::shared_ptr<GlobFilesResponse>>
SaplingNativeBackingStore::getGlobFiles(
    // Human Readable 40b commit id
    std::string_view commit_id,
    const std::vector<std::string>& suffixes,
    const std::vector<std::string>& prefixes) {
  rust::Vec<rust::String> rust_suffixes;
  rust::Vec<rust::String> rust_prefixes;
  std::copy(
      suffixes.begin(), suffixes.end(), std::back_inserter(rust_suffixes));
  std::copy(
      prefixes.begin(), prefixes.end(), std::back_inserter(rust_prefixes));

  auto br = folly::ByteRange(commit_id);
  return folly::makeTryWith([&] {
    auto globFiles = sapling_backingstore_get_glob_files(
        *store_.get(),
        rust::Slice<const uint8_t>{br.data(), br.size()},
        rust_suffixes,
        rust_prefixes);

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
