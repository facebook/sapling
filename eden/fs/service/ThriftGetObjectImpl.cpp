/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/ThriftGetObjectImpl.h"

#include "folly/Try.h"

#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/model/BlobAuxData.h"
#include "eden/fs/model/ObjectId.h"
#include "eden/fs/service/ThriftUtil.h"

namespace facebook::eden {
namespace detail {
folly::Try<BlobAuxData> transformToTryAuxData(
    folly::Try<std::optional<BlobAuxData>> auxData,
    std::shared_ptr<EdenMount> edenMount,
    ObjectId id) {
  if (auxData.hasException()) {
    return folly::Try<BlobAuxData>{std::move(auxData).exception()};
  }
  return transformToTryAuxData(auxData.value(), std::move(edenMount), id);
}
} // namespace detail

BlobMetadataWithOrigin transformToBlobMetadataFromOrigin(
    folly::Try<BlobAuxData> auxData,
    DataFetchOrigin origin) {
  BlobMetadataOrError auxDataOrError;
  if (auxData.hasValue()) {
    ScmBlobMetadata thriftMetadata;
    thriftMetadata.size() = auxData.value().size;
    thriftMetadata.contentsSha1() = thriftHash20(auxData.value().sha1);
    auxDataOrError.metadata_ref() = std::move(thriftMetadata);
  } else {
    auxDataOrError.error_ref() = newEdenError(auxData.exception());
  }
  BlobMetadataWithOrigin result;
  result.metadata() = std::move(auxDataOrError);
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
