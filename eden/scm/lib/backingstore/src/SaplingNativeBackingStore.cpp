/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#include "eden/scm/lib/backingstore/include/SaplingNativeBackingStore.h"

#include "eden/common/utils/CaseSensitivity.h"
#include "eden/fs/model/TreeFwd.h"
#include "eden/scm/lib/backingstore/include/SaplingBackingStoreError.h"
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
#include <type_traits>

namespace sapling {

SaplingNativeBackingStore::SaplingNativeBackingStore(
    std::string_view repository,
    std::string_view mount,
    facebook::eden::HgObjectIdFormat objectIdFormat,
    facebook::eden::CaseSensitivity caseSensitive)
    : store_{
          sapling_backingstore_new(
              rust::Slice<const char>{repository.data(), repository.size()},
              rust::Slice<const char>{mount.data(), mount.size()})
              .into_raw(),
          [](BackingStore* backingStore) {
            auto box = rust::Box<BackingStore>::from_raw(backingStore);
          }}, objectIdFormat_{objectIdFormat}, caseSensitive_{caseSensitive} {
  try {
    repoName_ = std::string(sapling_backingstore_get_name(*store_.get()));
  } catch (const rust::Error& error) {
    XLOGF(DBG2, "Error while repo name from backingstore: {}", error.what());
  }
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
    try {
      return sapling_backingstore_get_file_aux(
          *store_.get(),
          rust::Slice<const uint8_t>{node.data(), node.size()},
          fetch_mode);
    } catch (const rust::Error& error) {
      throw SaplingBackingStoreError{error.what()};
    }
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
    raw_requests.push_back(
        Request{
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
    try {
      auto globFiles = sapling_backingstore_get_glob_files(
          *store_.get(),
          rust::Slice<const uint8_t>{br.data(), br.size()},
          rust_suffixes,
          rust_prefixes);

      XCHECK(
          globFiles.get(),
          "sapling_backingstore_get_glob_files returned a nullptr, but did not throw an exception.");
      return globFiles;
    } catch (const rust::Error& error) {
      throw SaplingBackingStoreError{error.what()};
    }
  });
}

void SaplingNativeBackingStore::flush() {
  XLOG(DBG7, "Flushing backing store");

  sapling_backingstore_flush(*store_.get());
}

} // namespace sapling
