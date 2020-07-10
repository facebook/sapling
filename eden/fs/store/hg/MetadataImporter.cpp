/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/hg/MetadataImporter.h"

#include "eden/fs/model/Hash.h"
#include "eden/fs/store/TreeMetadata.h"

namespace facebook {
namespace eden {

folly::SemiFuture<std::unique_ptr<TreeMetadata>>
DefaultMetadataImporter::getTreeMetadata(const Hash& /*edenId*/) {
  return folly::SemiFuture<std::unique_ptr<TreeMetadata>>::makeEmpty();
};

bool DefaultMetadataImporter::metadataFetchingAvailable() {
  return false;
}

} // namespace eden
} // namespace facebook
