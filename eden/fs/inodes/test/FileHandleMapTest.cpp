/*
 *  Copyright (c) 2017-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include <folly/experimental/TestUtil.h>
#include <gtest/gtest.h>
#include "eden/fs/fuse/DirHandle.h"
#include "eden/fs/fuse/FileHandle.h"
#include "eden/fs/fuse/FileHandleMap.h"
#include "eden/fs/fuse/gen-cpp2/handlemap_types.h"

using namespace facebook::eden;
using namespace facebook::eden::fusell;
using folly::Future;
using folly::test::TemporaryFile;

namespace {

class FakeDirHandle : public DirHandle {
 public:
  explicit FakeDirHandle(fuse_ino_t inode) : inode_(inode) {}

  fuse_ino_t getInodeNumber() override {
    return inode_;
  }
  folly::Future<Dispatcher::Attr> getattr() override {
    throw std::runtime_error("fake!");
  }
  folly::Future<Dispatcher::Attr> setattr(const struct stat& attr, int to_set)
      override {
    throw std::runtime_error("fake!");
  }
  folly::Future<DirList> readdir(DirList&& list, off_t off) override {
    throw std::runtime_error("fake!");
  }

  folly::Future<folly::Unit> fsyncdir(bool datasync) override {
    throw std::runtime_error("fake!");
  }

 private:
  fuse_ino_t inode_;
};

class FakeFileHandle : public FileHandle {
 public:
  explicit FakeFileHandle(fuse_ino_t inode) : inode_(inode) {}

  fuse_ino_t getInodeNumber() override {
    return inode_;
  }
  folly::Future<Dispatcher::Attr> getattr() override {
    throw std::runtime_error("fake!");
  }
  folly::Future<Dispatcher::Attr> setattr(const struct stat& attr, int to_set)
      override {
    throw std::runtime_error("fake!");
  }

  folly::Future<BufVec> read(size_t size, off_t off) override {
    throw std::runtime_error("fake!");
  }
  folly::Future<size_t> write(BufVec&& buf, off_t off) override {
    throw std::runtime_error("fake!");
  }
  folly::Future<size_t> write(folly::StringPiece data, off_t off) override {
    throw std::runtime_error("fake!");
  }
  folly::Future<folly::Unit> flush(uint64_t lock_owner) override {
    throw std::runtime_error("fake!");
  }
  folly::Future<folly::Unit> fsync(bool datasync) override {
    throw std::runtime_error("fake!");
  }

 private:
  fuse_ino_t inode_;
};
}

FileHandleMapEntry makeEntry(fuse_ino_t inode, uint64_t handleId, bool isDir) {
  FileHandleMapEntry entry;
  entry.inodeNumber = inode;
  entry.handleId = (int64_t)handleId;
  entry.isDir = isDir;
  return entry;
}

TEST(FileHandleMap, Serialization) {
  FileHandleMap fmap;

  auto fileHandle = std::make_shared<FakeFileHandle>(123);
  auto dirHandle = std::make_shared<FakeDirHandle>(345);

  auto fileHandleNo = fmap.recordHandle(fileHandle);
  auto dirHandleNo = fmap.recordHandle(dirHandle);

  auto serialized = fmap.serializeMap();

  std::sort(
      serialized.entries.begin(),
      serialized.entries.end(),
      [](const FileHandleMapEntry& a, const FileHandleMapEntry& b) {
        return a.inodeNumber < b.inodeNumber;
      });

  std::vector<FileHandleMapEntry> expected = {
      makeEntry(123, fileHandleNo, false), makeEntry(345, dirHandleNo, true)};

  EXPECT_EQ(expected, serialized.entries);

  TemporaryFile mapFile("file-handles");
  fmap.saveFileHandleMap(mapFile.path().c_str());

  auto loaded = FileHandleMap::loadFileHandleMap(mapFile.path().c_str());

  std::sort(
      loaded.entries.begin(),
      loaded.entries.end(),
      [](const FileHandleMapEntry& a, const FileHandleMapEntry& b) {
        return a.inodeNumber < b.inodeNumber;
      });

  EXPECT_EQ(expected, loaded.entries);

  FileHandleMap newMap;
  newMap.recordHandle(fileHandle, fileHandleNo);
  newMap.recordHandle(dirHandle, dirHandleNo);

  auto newSerialized = fmap.serializeMap();

  std::sort(
      newSerialized.entries.begin(),
      newSerialized.entries.end(),
      [](const FileHandleMapEntry& a, const FileHandleMapEntry& b) {
        return a.inodeNumber < b.inodeNumber;
      });

  EXPECT_EQ(expected, newSerialized.entries);
}
