/*
 *  Copyright (c) 2016, Facebook, Inc.
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

namespace facebook {
namespace eden {

using apache::thrift::CompactSerializer;

void DirstatePersistence::save(
    const std::unordered_map<RelativePath, overlay::UserStatusDirective>&
        userDirectives) {
  overlay::DirstateData dirstateData;
  std::map<std::string, overlay::UserStatusDirective> directives;
  for (auto& pair : userDirectives) {
    directives[pair.first.stringPiece().str()] = pair.second;
  }
  dirstateData.directives = directives;
  auto serializedData = CompactSerializer::serialize<std::string>(dirstateData);
  auto wrote = folly::writeFile(serializedData, storageFile_.c_str());

  if (!wrote) {
    throw std::runtime_error(folly::to<std::string>(
        "Failed to persist Dirstate to file ", storageFile_));
  }
}

std::unordered_map<RelativePath, overlay::UserStatusDirective>
DirstatePersistence::load() {
  std::string serializedData;
  std::unordered_map<RelativePath, overlay::UserStatusDirective> entries;
  if (!folly::readFile(storageFile_.c_str(), serializedData)) {
    int err = errno;
    if (err == ENOENT) {
      return entries;
    }
    folly::throwSystemErrorExplicit(err, "failed to read ", storageFile_);
  }

  auto dirstateData =
      CompactSerializer::deserialize<overlay::DirstateData>(serializedData);
  for (auto& pair : dirstateData.directives) {
    entries[RelativePath(pair.first)] = pair.second;
  }

  return entries;
}
}
}
