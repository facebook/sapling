/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "folly/Try.h"

#include "eden/common/utils/OptionSet.h"

#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/model/ObjectId.h"
#include "eden/fs/service/gen-cpp2/eden_types.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/utils/EdenError.h"

namespace folly {
template <typename T>
class Try;
}

namespace facebook::eden {
class Blob;
class BlobMetadata;

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

template <typename T>
ScmBlobWithOrigin transformToBlobFromOrigin(
    std::shared_ptr<EdenMount> edenMount,
    ObjectId id,
    folly::Try<T> blob,
    DataFetchOrigin origin) {
  ScmBlobOrError blobOrError;
  if (blob.hasValue()) {
    if (!blob.value()) {
      blobOrError.error_ref() = newEdenError(
          ENOENT,
          EdenErrorType::POSIX_ERROR,
          "no blob found for id ",
          edenMount->getObjectStore()->renderObjectId(id));
    } else {
      blobOrError.blob_ref() = blob.value()->asString();
    }
  } else {
    blobOrError.error_ref() = newEdenError(blob.exception());
  }
  ScmBlobWithOrigin result;
  result.blob() = std::move(blobOrError);
  result.origin() = std::move(origin);
  return result;
}

namespace detail {
/**
 * Our various methods to get blob metadata through out EdenFS
 * return different types. In fact, no blob metadata fetching method returns the
 * same type as another :( `transformToTryMetadata` converts some BlobMetadata
 * type into a Try<BlobMetadata>. this is an intermediary for converting the
 * data into our thrift type (BlobMetadataWithOrigin).
 */

template <typename T>
folly::Try<BlobMetadata> transformToTryMetadata(
    T metadata,
    std::shared_ptr<EdenMount> edenMount,
    ObjectId id) {
  if (metadata) {
    return folly::Try<BlobMetadata>{std::move(*metadata)};
  }
  return folly::Try<BlobMetadata>{newEdenError(
      ENOENT,
      EdenErrorType::POSIX_ERROR,
      "no blob found for id ",
      edenMount->getObjectStore()->renderObjectId(id))};
}

// [[maybe_unused]]: This specialization is used and nessecary, but clang's
// maybe unused thing thinks that the templated transformToTryMetadata above
// will over shadow this specialization. So clang will think this is unused.
// Apparently, clang does not bother trying to instantate a templated thing.
// So its prone to false positive "unused" warnings with tempated stuff.
// (source:
// https://stackoverflow.com/questions/66986718/c-clang-emit-warning-about-unused-template-variable)
// Maybe concepts in C++20 will clear this up, but we aren't there yet.
template <>
[[maybe_unused]] folly::Try<BlobMetadata> transformToTryMetadata(
    folly::Try<std::optional<BlobMetadata>> metadata,
    std::shared_ptr<EdenMount> edenMount,
    ObjectId id);
} // namespace detail

/**
 * Transforms BlobMetadata in some format into a BlobMetadataWithOrigin.
 */
template <typename T>
BlobMetadataWithOrigin transformToBlobMetadataFromOrigin(
    std::shared_ptr<EdenMount> edenMount,
    ObjectId id,
    T raw_metadata,
    DataFetchOrigin origin) {
  auto metadata =
      detail::transformToTryMetadata(std::move(raw_metadata), edenMount, id);
  return transformToBlobMetadataFromOrigin(std::move(metadata), origin);
}

BlobMetadataWithOrigin transformToBlobMetadataFromOrigin(
    folly::Try<BlobMetadata> metadata,
    DataFetchOrigin origin);
} // namespace facebook::eden
