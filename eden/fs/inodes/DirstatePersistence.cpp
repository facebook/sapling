/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "DirstatePersistence.h"

#include <folly/FileUtil.h>
#include <thrift/lib/cpp2/protocol/Serializer.h>
#include "eden/fs/inodes/gen-cpp2/overlay_types.h"

namespace facebook {
namespace eden {

using apache::thrift::CompactSerializer;

void DirstatePersistence::save(const DirstateData& data) {
  save(data, storageFile_);
}

void DirstatePersistence::save(
    const DirstateData& data,
    const AbsolutePath& storageFile) {
  std::map<std::string, hgdirstate::DirstateTuple> hgDirstateTuples;
  for (auto& pair : data.hgDirstateTuples) {
    hgDirstateTuples.emplace(pair.first.str(), pair.second);
  }

  std::map<std::string, std::string> hgDestToSourceCopyMap;
  for (auto& pair : data.hgDestToSourceCopyMap) {
    hgDestToSourceCopyMap.emplace(
        pair.first.str(), pair.second.stringPiece().str());
  }

  save(storageFile, hgDirstateTuples, hgDestToSourceCopyMap);
}

void DirstatePersistence::save(
    const AbsolutePath& storageFile,
    const std::map<std::string, hgdirstate::DirstateTuple>& hgDirstateTuples,
    const std::map<std::string, std::string>& hgDestToSourceCopyMap) {
  overlay::DirstateData dirstateData;
  dirstateData.hgDirstateTuples = hgDirstateTuples;
  dirstateData.__isset.hgDirstateTuples = true;
  dirstateData.hgDestToSourceCopyMap = hgDestToSourceCopyMap;
  dirstateData.__isset.hgDestToSourceCopyMap = true;
  auto serializedData = CompactSerializer::serialize<std::string>(dirstateData);
  folly::writeFileAtomic(storageFile.stringPiece(), serializedData, 0644);
}

DirstateData DirstatePersistence::load() {
  return load(storageFile_);
}

DirstateData DirstatePersistence::load(const AbsolutePath& storageFile) {
  DirstateData loadedData;
  std::string serializedData;
  if (!folly::readFile(storageFile.c_str(), serializedData)) {
    int err = errno;
    if (err == ENOENT) {
      return loadedData;
    }
    folly::throwSystemErrorExplicit(err, "failed to read ", storageFile);
  }

  auto dirstateData =
      CompactSerializer::deserialize<overlay::DirstateData>(serializedData);
  for (const auto& pair : dirstateData.get_hgDirstateTuples()) {
    loadedData.hgDirstateTuples.emplace(pair.first, pair.second);
  }
  for (const auto& pair : dirstateData.get_hgDestToSourceCopyMap()) {
    loadedData.hgDestToSourceCopyMap.emplace(
        pair.first, RelativePath{pair.second});
  }
  return loadedData;
}
} // namespace eden
} // namespace facebook
