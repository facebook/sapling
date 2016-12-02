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
#include "eden/fs/inodes/Dirstate.h"
#include "eden/fs/inodes/DirstatePersistence.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/gen-cpp2/overlay_types.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/utils/PathFuncs.h"

namespace facebook {
namespace eden {
class FileInode;
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
      std::unique_ptr<folly::test::TemporaryDirectory> testDir)
      : edenMount_(edenMount), testDir_(std::move(testDir)) {}

  /**
   * Add file to the mount; it will be available in the overlay.
   */
  void addFile(folly::StringPiece path, std::string contents);

  void mkdir(folly::StringPiece path);

  /** Overwrites the contents of an existing file. */
  void overwriteFile(folly::StringPiece path, std::string contents);

  std::string readFile(folly::StringPiece path);

  /** Returns true if path identifies a regular file in the tree. */
  bool hasFileAt(folly::StringPiece path);

  void deleteFile(folly::StringPiece path);
  void rmdir(folly::StringPiece path);

  std::shared_ptr<TreeInode> getTreeInode(RelativePathPiece path) const;
  std::shared_ptr<TreeInode> getTreeInode(folly::StringPiece path) const;
  std::shared_ptr<FileInode> getFileInode(RelativePathPiece path) const;
  std::shared_ptr<FileInode> getFileInode(folly::StringPiece path) const;

  /** Convenience method for getting the Tree for the root of the mount. */
  std::unique_ptr<Tree> getRootTree() const;

  std::shared_ptr<EdenMount> getEdenMount() {
    return edenMount_;
  }

  Dirstate* getDirstate() const;

 private:
  std::shared_ptr<EdenMount> edenMount_;

  // The TestMount must hold onto the test TemporaryDirectory because it needs
  // to live for the duration of the test.
  std::unique_ptr<folly::test::TemporaryDirectory> testDir_;
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

  void addUserDirectives(
      std::unordered_map<RelativePath, overlay::UserStatusDirective>&&
          userDirectives);

 private:
  /** Populate the test client directory, and return a ClientConfig obeject */
  std::unique_ptr<ClientConfig> setupClientConfig(
      AbsolutePathPiece testDirectory,
      Hash rootTreeHash);

  std::vector<TestMountFile> files_;
  std::unordered_map<RelativePath, overlay::UserStatusDirective>
      userDirectives_;
};
}
}
