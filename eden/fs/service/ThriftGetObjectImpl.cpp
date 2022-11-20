/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/ThriftGetObjectImpl.h"

#include "folly/Try.h"

#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/ObjectId.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/utils/EdenError.h"

namespace facebook::eden {
ScmBlobWithOrigin getBlobFromOrigin(
    std::shared_ptr<EdenMount> edenMount,
    ObjectId id,
    folly::Try<std::shared_ptr<const Blob>> blob,
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

ScmBlobWithOrigin getBlobFromOrigin(
    std::shared_ptr<EdenMount> edenMount,
    ObjectId id,
    folly::Try<std::unique_ptr<Blob>> blob,
    DataFetchOrigin origin) {
  if (blob.hasException()) {
    return getBlobFromOrigin(
        std::move(edenMount),
        std::move(id),
        folly::Try<std::shared_ptr<const Blob>>(std::move(blob).exception()),
        origin);
  }
  std::shared_ptr<Blob> shared_blob = std::move(blob.value());
  return getBlobFromOrigin(
      std::move(edenMount),
      std::move(id),
      folly::Try<std::shared_ptr<const Blob>>{std::move(shared_blob)},
      origin);
}

ScmBlobWithOrigin getBlobFromOrigin(
    std::shared_ptr<EdenMount> edenMount,
    ObjectId id,
    folly::Try<std::shared_ptr<Blob>> blob,
    DataFetchOrigin origin) {
  if (blob.hasException()) {
    return getBlobFromOrigin(
        std::move(edenMount),
        std::move(id),
        folly::Try<std::shared_ptr<const Blob>>(std::move(blob).exception()),
        origin);
  }
  return getBlobFromOrigin(
      std::move(edenMount),
      std::move(id),
      folly::Try<std::shared_ptr<const Blob>>{std::move(blob.value())},
      origin);
}

} // namespace facebook::eden
