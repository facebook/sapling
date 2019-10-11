/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "folly/portability/Windows.h"

#include <ProjectedFSLib.h>
#include "eden/fs/win/mount/Enumerator.h"
#include "eden/fs/win/store/WinStore.h"

namespace facebook {
namespace eden {

Enumerator::Enumerator(
    const GUID& enumerationId,
    const std::wstring& path,
    std::vector<FileMetadata> entryList)
    : path_(path), metadataList_(std::move(entryList)) {
  std::sort(
      metadataList_.begin(),
      metadataList_.end(),
      [](const FileMetadata& first, const FileMetadata& second) -> bool {
        return (
            PrjFileNameCompare(first.name.c_str(), second.name.c_str()) < 0);
      });
}

const FileMetadata* Enumerator::current() {
  for (; listIndex_ < metadataList_.size(); listIndex_++) {
    DCHECK(!searchExpression_.empty());
    if (PrjFileNameMatch(
            metadataList_[listIndex_].name.c_str(),
            searchExpression_.c_str())) {
      //
      // Don't increment the index here because we don't know if the caller
      // would be able to use this. The caller should instead call advance() on
      // success.
      //
      return &metadataList_[listIndex_];
    }
  }
  return nullptr;
}

} // namespace eden
} // namespace facebook
