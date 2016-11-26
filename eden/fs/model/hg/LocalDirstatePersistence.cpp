/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/model/hg/LocalDirstatePersistence.h"
#include <folly/FileUtil.h>
#include <thrift/lib/cpp2/protocol/Serializer.h>
#include "eden/fs/model/hg/gen-cpp2/dirstate_types.h"

namespace facebook {
namespace eden {

using apache::thrift::CompactSerializer;

void LocalDirstatePersistence::save(
    const std::unordered_map<RelativePath, HgUserStatusDirective>&
        userDirectives) {
  dirstate::DirstateData dirstateData;
  std::map<std::string, dirstate::HgUserStatusDirectiveValue> directives;
  for (auto& pair : userDirectives) {
    dirstate::HgUserStatusDirectiveValue value;
    switch (pair.second) {
      case HgUserStatusDirective::ADD:
        value = dirstate::HgUserStatusDirectiveValue::Add;
        break;
      case HgUserStatusDirective::REMOVE:
        value = dirstate::HgUserStatusDirectiveValue::Remove;
        break;
    }
    directives[pair.first.stringPiece().str()] = value;
  }
  dirstateData.directives = directives;
  auto serializedData = CompactSerializer::serialize<std::string>(dirstateData);
  auto wrote = folly::writeFile(serializedData, storageFile_.c_str());

  if (!wrote) {
    throw std::runtime_error(folly::to<std::string>(
        "Failed to persist Dirstate to file ", storageFile_));
  }
}

std::unordered_map<RelativePath, HgUserStatusDirective>
LocalDirstatePersistence::load() {
  std::string serializedData;
  std::unordered_map<RelativePath, HgUserStatusDirective> entries;
  if (!folly::readFile(storageFile_.c_str(), serializedData)) {
    int err = errno;
    if (err == ENOENT) {
      return entries;
    }
    folly::throwSystemErrorExplicit(err, "failed to read ", storageFile_);
  }

  auto dirstateData =
      CompactSerializer::deserialize<dirstate::DirstateData>(serializedData);
  for (auto& pair : dirstateData.directives) {
    HgUserStatusDirective directive;
    switch (pair.second) {
      case dirstate::HgUserStatusDirectiveValue::Add:
        directive = HgUserStatusDirective::ADD;
        break;
      case dirstate::HgUserStatusDirectiveValue::Remove:
        directive = HgUserStatusDirective::REMOVE;
        break;
      default:
        throw std::runtime_error(
            "Unexpected value loaded for HgUserStatusDirectiveValue");
    }
    entries[RelativePath(pair.first)] = directive;
  }

  return entries;
}
}
}
