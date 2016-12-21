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
#include "eden/fs/inodes/InodePtr.h"
#include "eden/fs/inodes/gen-cpp2/overlay_types.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/utils/PathFuncs.h"

namespace facebook {
namespace eden {
class ClientConfig;
class FakeBackingStore;
class FileInode;
class LocalStore;
class TreeInode;
template <typename T>
class StoredObject;
using StoredHash = StoredObject<Hash>;

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
      std::unique_ptr<folly::test::TemporaryDirectory> testDir);
  ~TestMount();

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

  TreeInodePtr getTreeInode(RelativePathPiece path) const;
  TreeInodePtr getTreeInode(folly::StringPiece path) const;
  FileInodePtr getFileInode(RelativePathPiece path) const;
  FileInodePtr getFileInode(folly::StringPiece path) const;

  /** Convenience method for getting the Tree for the root of the mount. */
  std::unique_ptr<Tree> getRootTree() const;

  std::shared_ptr<EdenMount> getEdenMount() {
    return edenMount_;
  }

  Dirstate* getDirstate() const;

 private:
  /**
   * The temporary directory for this TestMount.
   *
   * This must be stored as a member variable to ensure the temporary directory
   * lives for the duration of the test.
   *
   * We intentionally list it before the edenMount_ so it gets constructed
   * first, and destroyed (and deleted from disk) after the EdenMount is
   * destroyed.
   */
  std::unique_ptr<folly::test::TemporaryDirectory> testDir_;

  std::shared_ptr<EdenMount> edenMount_;
};

/**
 * A class for helping construct the temporary directories and other state
 * needed to create a TestMount.
 */
class BaseTestMountBuilder {
 public:
  BaseTestMountBuilder();
  virtual ~BaseTestMountBuilder();

  /**
   * Build the TestMount.
   *
   * This BaseTestMountBuilder object should not be accessed again after
   * calling build().
   */
  std::unique_ptr<TestMount> build();

  /**
   * Get the ClientConfig object.
   *
   * The ClientConfig object provides methods to get the paths to the mount
   * point, the client directory, etc.
   */
  ClientConfig* getConfig() const {
    return config_.get();
  }

  /**
   * Get the LocalStore.
   *
   * Callers can use this to populate the LocalStore before calling build().
   */
  const std::shared_ptr<LocalStore>& getLocalStore() const {
    return localStore_;
  }

  /**
   * Get the LocalStore.
   *
   * Callers can use this to populate the BackingStore before calling build().
   */
  const std::shared_ptr<FakeBackingStore>& getBackingStore() const {
    return backingStore_;
  }

  /**
   * Store a commit ID to tree ID mapping in the BackingStore,
   * and then write this commit ID to the current SNAPSHOT file.
   *
   * This automatically makes the commit ID --> tree ID mapping ready in the
   * FakeBackingStore.
   *
   * Note that the EdenMount constructor currently blocks until the root tree
   * is ready, so the caller must make the returned StoredHash ready before
   * calling build.  The Tree referenced by the rootTreeHash must also be
   * ready or available in the LocalStore.
   */
  void setCommit(Hash commitHash, Hash rootTreeHash);

  /**
   * Write a commit ID to the current SNAPSHOT file.
   *
   * Note that the EdenMount constructor currently blocks until the root tree
   * is ready, so the caller must ensure that the commit hash and root tree are
   * both ready in the ObjectStore.
   */
  void writeSnapshotFile(Hash commitHash);

 private:
  void initTestDirectory();

  virtual void populateStore();

  std::unique_ptr<folly::test::TemporaryDirectory> testDir_;
  std::unique_ptr<ClientConfig> config_;
  std::shared_ptr<LocalStore> localStore_;
  std::shared_ptr<FakeBackingStore> backingStore_;
};

/**
 * An implementation of BaseTestMountBuilder that helps populate the
 * LocalStore.
 *
 * All files defined with addFile()/addFiles() are added directly to the
 * LocalStore.  They will therefore always be immediately available, and the
 * test code cannot control when their Futures are fulfilled.
 */
class TestMountBuilder : public BaseTestMountBuilder {
 public:
  TestMountBuilder();
  virtual ~TestMountBuilder();

  void addFile(TestMountFile&& file) {
    files_.emplace_back(std::move(file));
  }

  void addFiles(std::vector<TestMountFile>&& files) {
    for (auto&& f : files) {
      files_.emplace_back(std::move(f));
    }
  }

  void addUserDirectives(
      const std::unordered_map<RelativePath, overlay::UserStatusDirective>&
          userDirectives);

 private:
  void populateStore() override;

  std::vector<TestMountFile> files_;
  std::unordered_map<RelativePath, overlay::UserStatusDirective>
      userDirectives_;
};
}
}
