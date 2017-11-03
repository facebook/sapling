/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/fuse/FileHandleMap.h"

#include <folly/Exception.h>
#include <folly/FileUtil.h>
#include <folly/Random.h>
#include <folly/experimental/logging/xlog.h>
#include <thrift/lib/cpp2/protocol/Serializer.h>

#include "eden/fs/fuse/DirHandle.h"
#include "eden/fs/fuse/FileHandle.h"
#include "eden/fs/fuse/gen-cpp2/handlemap_types.h"

using apache::thrift::CompactSerializer;

namespace facebook {
namespace eden {
namespace fusell {

std::shared_ptr<FileHandleBase> FileHandleMap::getGenericFileHandle(
    uint64_t fh) {
  auto handles = handles_.rlock();
  auto iter = handles->find(fh);
  if (iter == handles->end()) {
    folly::throwSystemErrorExplicit(
        EBADF, "file number ", fh, " is not tracked by this FileHandleMap");
  }
  return iter->second;
}

std::shared_ptr<FileHandle> FileHandleMap::getFileHandle(uint64_t fh) {
  auto handle = getGenericFileHandle(fh);
  auto result = std::dynamic_pointer_cast<FileHandle>(handle);
  if (result) {
    return result;
  }
  folly::throwSystemErrorExplicit(
      EISDIR, "file number ", fh, " is a DirHandle, not a FileHandle");
}

std::shared_ptr<DirHandle> FileHandleMap::getDirHandle(uint64_t fh) {
  auto handle = getGenericFileHandle(fh);
  auto result = std::dynamic_pointer_cast<DirHandle>(handle);
  if (result) {
    return result;
  }
  folly::throwSystemErrorExplicit(
      ENOTDIR, "file number ", fh, " is a FileHandle, not a DirHandle");
}

void FileHandleMap::recordHandle(
    std::shared_ptr<FileHandleBase> fh,
    uint64_t number) {
  auto handles = handles_.wlock();

  auto it = handles->find(number);
  if (it != handles->end()) {
    folly::throwSystemErrorExplicit(
        EEXIST, "file number ", number, " is already present in the map!?");
  }

  handles->emplace(number, fh);
}

uint64_t FileHandleMap::recordHandle(std::shared_ptr<FileHandleBase> fh) {
  auto handles = handles_.wlock();

  // Our assignment strategy is just to take the address of the instance
  // and return that as a 64-bit number.  This avoids needing to use
  // any other mechanism for assigning or tracking numbers and keeps the
  // cost of the assignment constant.
  //
  // However, in the future hot upgrade case, we need to be able to pass
  // the mapping from another process where there is no way for us to
  // contrive an address for a given instance.
  //
  // So what we do it first try to take the address from the incoming
  // file handle, but if we get a collision we fall back to attempting
  // a random assignment a reasonable number of times.  This is similar
  // to the AUTOINCREMENT behavior in sqlite.
  //
  // The collision handling scenario should be pretty rare.

  auto number = reinterpret_cast<uint64_t>(fh.get());
  for (auto attempts = 0; attempts < 100; ++attempts) {
    auto& entry = (*handles)[number];

    if (LIKELY(!entry)) {
      // Successfully inserted with no collision
      entry = std::move(fh);
      return number;
    }

    // There was a collision, we try at random for a bounded number of
    // attempts.  100 was picked as a reasonable number of tries and is
    // the same number used by sqlite in a similar situation.
    number = folly::Random::rand64();
  }

  // Fail this request with a reasonable approximation of the problem
  XLOG(ERR) << "Unable to find a usable file number within "
               "a reasonable number of attempts";
  folly::throwSystemErrorExplicit(EMFILE);
}

std::shared_ptr<FileHandleBase> FileHandleMap::forgetGenericHandle(
    uint64_t fh) {
  auto handles = handles_.wlock();

  auto iter = handles->find(fh);
  if (iter == handles->end()) {
    folly::throwSystemErrorExplicit(EBADF);
  }
  auto result = iter->second;
  handles->erase(iter);
  return result;
}

SerializedFileHandleMap FileHandleMap::serializeMap() const {
  SerializedFileHandleMap result;

  auto handles = handles_.rlock();
  for (auto& it : *handles) {
    FileHandleMapEntry entry;

    entry.handleId = (int64_t)it.first;
    entry.isDir = std::dynamic_pointer_cast<DirHandle>(it.second) != nullptr;
    entry.inodeNumber = it.second->getInodeNumber();

    result.entries.push_back(std::move(entry));
  }

  return result;
}

void FileHandleMap::saveFileHandleMap(folly::StringPiece fileName) const {
  auto serializedData =
      CompactSerializer::serialize<std::string>(serializeMap());

  // And update the file on disk
  folly::writeFileAtomic(fileName, serializedData);
}

SerializedFileHandleMap FileHandleMap::loadFileHandleMap(
    folly::StringPiece fileName) {
  auto path = fileName.str();
  std::string serializedData;
  if (!folly::readFile(path.c_str(), serializedData)) {
    folly::throwSystemError("failed to read FileHandleMap from ", path);
  }

  return CompactSerializer::deserialize<SerializedFileHandleMap>(
      serializedData);
}
} // namespace fusell
} // namespace eden
} // namespace facebook
