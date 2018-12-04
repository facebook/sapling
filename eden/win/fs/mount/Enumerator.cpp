/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */

#include "folly/portability/Windows.h"

#include <ProjectedFSLib.h>
#include "eden/win/fs/mount/Enumerator.h"
#include "eden/win/fs/store/WinStore.h"

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
