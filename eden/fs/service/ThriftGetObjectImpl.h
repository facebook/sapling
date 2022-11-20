/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/common/utils/OptionSet.h"

#include "eden/fs/service/gen-cpp2/eden_types.h"

namespace folly {
template <typename T>
class Try;
}

namespace facebook::eden {
class EdenMount;
class ObjectId;
class Blob;

struct DataFetchOriginFlags
    : OptionSet<DataFetchOriginFlags, std::underlying_type_t<DataFetchOrigin>> {
  constexpr static DataFetchOriginFlags raw(DataFetchOrigin raw) {
    return OptionSet<
        DataFetchOriginFlags,
        std::underlying_type_t<DataFetchOrigin>>::
        raw(folly::to_underlying(raw));
  }
  constexpr static DataFetchOriginFlags raw(
      std::underlying_type_t<DataFetchOrigin> raw) {
    return OptionSet<
        DataFetchOriginFlags,
        std::underlying_type_t<DataFetchOrigin>>::raw(raw);
  }
};

inline constexpr auto FROMWHERE_MEMORY_CACHE =
    DataFetchOriginFlags::raw(DataFetchOrigin::MEMORY_CACHE);
inline constexpr auto FROMWHERE_DISK_CACHE =
    DataFetchOriginFlags::raw(DataFetchOrigin::DISK_CACHE);
inline constexpr auto FROMWHERE_LOCAL_BACKING_STORE =
    DataFetchOriginFlags::raw(DataFetchOrigin::LOCAL_BACKING_STORE);
inline constexpr auto FROMWHERE_REMOTE_BACKING_STORE =
    DataFetchOriginFlags::raw(DataFetchOrigin::REMOTE_BACKING_STORE);
inline constexpr auto FROMWHERE_ANYWHERE =
    DataFetchOriginFlags::raw(DataFetchOrigin::ANYWHERE);

ScmBlobWithOrigin getBlobFromOrigin(
    std::shared_ptr<EdenMount> edenMount,
    ObjectId id,
    folly::Try<std::shared_ptr<const Blob>> blobFuture,
    DataFetchOrigin origin);

ScmBlobWithOrigin getBlobFromOrigin(
    std::shared_ptr<EdenMount> edenMount,
    ObjectId id,
    folly::Try<std::unique_ptr<Blob>> blobFuture,
    DataFetchOrigin origin);

ScmBlobWithOrigin getBlobFromOrigin(
    std::shared_ptr<EdenMount> edenMount,
    ObjectId id,
    folly::Try<std::shared_ptr<Blob>> blobFuture,
    DataFetchOrigin origin);
} // namespace facebook::eden
