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
#include "eden/fs/model/BlobMetadata.h"
#include "eden/fs/model/ObjectId.h"
#include "eden/fs/service/ThriftUtil.h"

namespace facebook::eden {
namespace detail {
folly::Try<BlobMetadata> transformToTryMetadata(
    folly::Try<std::optional<BlobMetadata>> metadata,
    std::shared_ptr<EdenMount> edenMount,
    ObjectId id) {
  if (metadata.hasException()) {
    return folly::Try<BlobMetadata>{std::move(metadata).exception()};
  }
  return transformToTryMetadata(metadata.value(), std::move(edenMount), id);
}
} // namespace detail

BlobMetadataWithOrigin transformToBlobMetadataFromOrigin(
    folly::Try<BlobMetadata> metadata,
    DataFetchOrigin origin) {
  BlobMetadataOrError metadataOrError;
  if (metadata.hasValue()) {
    ScmBlobMetadata thriftMetadata;
    thriftMetadata.size() = metadata.value().size;
    thriftMetadata.contentsSha1() = thriftHash20(metadata.value().sha1);
    metadataOrError.metadata_ref() = std::move(thriftMetadata);
  } else {
    metadataOrError.error_ref() = newEdenError(metadata.exception());
  }
  BlobMetadataWithOrigin result;
  result.metadata() = std::move(metadataOrError);
  result.origin() = std::move(origin);
  return result;
}

ScmTreeWithOrigin transformToTreeFromOrigin(
    std::shared_ptr<EdenMount> edenMount,
    const ObjectId& id,
    const folly::Try<std::shared_ptr<const Tree>>& tree,
    DataFetchOrigin origin) {
  ScmTreeOrError treeOrError;
  if (tree.hasValue()) {
    if (!tree.value()) {
      treeOrError.error_ref() = newEdenError(
          ENOENT,
          EdenErrorType::POSIX_ERROR,
          "no tree found for id ",
          edenMount->getObjectStore()->renderObjectId(id));
    } else {
      for (const auto& entry : *(tree.value())) {
        const auto& [name, treeEntry] = entry;
        treeOrError.treeEntries_ref().ensure().emplace_back();
        auto& out = treeOrError.treeEntries_ref()->back();
        out.name_ref() = name.asString();
        out.mode_ref() = modeFromTreeEntryType(treeEntry.getType());
        out.id_ref() =
            edenMount->getObjectStore()->renderObjectId(treeEntry.getHash());
      }
    }
  } else {
    treeOrError.error_ref() = newEdenError(tree.exception());
  }
  ScmTreeWithOrigin result;
  result.scmTreeData() = std::move(treeOrError);
  result.origin() = std::move(origin);
  return result;
}

} // namespace facebook::eden
