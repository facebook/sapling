/*
 *  Copyright (c) 2017-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/fuse/FileHandleMap.h"
#include <folly/experimental/TestUtil.h>
#include <gtest/gtest.h>
#include "eden/fs/fuse/FileHandle.h"
#include "eden/fs/fuse/gen-cpp2/handlemap_types.h"

using namespace facebook::eden;
using folly::Future;
using folly::test::TemporaryFile;

namespace {

class FakeFileHandle : public FileHandle {
 public:
};
} // namespace

FileHandleMapEntry makeEntry(uint64_t inode, uint64_t handleId, bool isDir) {
  FileHandleMapEntry entry;
  entry.inodeNumber = inode;
  entry.handleId = (int64_t)handleId;
  entry.isDir = isDir;
  return entry;
}

TEST(FileHandleMap, Serialization) {
  FileHandleMap fmap;

  auto fileHandle = std::make_shared<FakeFileHandle>();

  auto fileHandleNo = fmap.recordHandle(fileHandle, 123_ino);

  auto serialized = fmap.serializeMap();

  std::sort(
      serialized.entries.begin(),
      serialized.entries.end(),
      [](const FileHandleMapEntry& a, const FileHandleMapEntry& b) {
        return a.inodeNumber < b.inodeNumber;
      });

  std::vector<FileHandleMapEntry> expected = {
      makeEntry(123, fileHandleNo, false)};

  EXPECT_EQ(expected, serialized.entries);

  FileHandleMap newMap;
  newMap.recordHandle(fileHandle, 123_ino, fileHandleNo);

  auto newSerialized = newMap.serializeMap();

  std::sort(
      newSerialized.entries.begin(),
      newSerialized.entries.end(),
      [](const FileHandleMapEntry& a, const FileHandleMapEntry& b) {
        return a.inodeNumber < b.inodeNumber;
      });

  EXPECT_EQ(expected, newSerialized.entries);
}
