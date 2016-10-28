/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <folly/Range.h>
#include <folly/experimental/TestUtil.h>
#include <sys/stat.h>
#include <vector>
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/utils/PathFuncs.h"

namespace facebook {
namespace eden {
class TreeEntryFileInode;
class TreeInode;

struct TestMountFile {
  RelativePath path;
  std::string contents;
  uint8_t rwx = 0b110;
  FileType type = FileType::REGULAR_FILE;

  /** Performs a structural equals comparison. */
  bool operator==(const TestMountFile& other) const;

  /**
   * @param path is a StringPiece (rather than a RelativePath) for convenience
   *     for creating instances of TestMountFile for unit tests.
   */
  TestMountFile(folly::StringPiece path, std::string contents)
      : path(path), contents(std::move(contents)) {}
};

class TestMount {
 public:
  TestMount(
      std::shared_ptr<EdenMount> edenMount,
      std::unique_ptr<folly::test::TemporaryDirectory> mountPointDir,
      std::unique_ptr<folly::test::TemporaryDirectory> pathToRocksDb,
      std::unique_ptr<folly::test::TemporaryDirectory> overlayDir)
      : edenMount_(edenMount),
        mountPointDir_(std::move(mountPointDir)),
        pathToRocksDb_(std::move(pathToRocksDb)),
        overlayDir_(std::move(overlayDir)) {}

  /**
   * Add file to the mount; it will be available in the overlay.
   */
  void addFile(folly::StringPiece path, std::string contents);

  void mkdir(folly::StringPiece path);

  /** Overwrites the contents of an existing file. */
  void overwriteFile(folly::StringPiece path, std::string contents);

  std::string readFile(folly::StringPiece path);

  void deleteFile(folly::StringPiece path);

  std::shared_ptr<TreeInode> getDirInodeForPath(folly::StringPiece path) const;
  std::shared_ptr<TreeEntryFileInode> getFileInodeForPath(
      folly::StringPiece path) const;

  /** Convenience method for getting the Tree for the root of the mount. */
  std::unique_ptr<Tree> getRootTree() const;

  std::shared_ptr<EdenMount> getEdenMount() {
    return edenMount_;
  }

 private:
  std::shared_ptr<EdenMount> edenMount_;

  // The TestMount must hold onto these TemporaryDirectories because they need
  // to live for the duration of the test.
  std::unique_ptr<folly::test::TemporaryDirectory> mountPointDir_;
  std::unique_ptr<folly::test::TemporaryDirectory> pathToRocksDb_;
  std::unique_ptr<folly::test::TemporaryDirectory> overlayDir_;
};

class TestMountBuilder {
 public:
  std::unique_ptr<TestMount> build();

  void addFile(TestMountFile&& file) {
    files_.emplace_back(std::move(file));
  }

  void addFiles(std::vector<TestMountFile>&& files) {
    for (auto&& f : files) {
      files_.emplace_back(std::move(f));
    }
  }

 private:
  std::vector<TestMountFile> files_;
};
}
}
