/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "TestMount.h"

#include <folly/FileUtil.h>
#include <folly/experimental/TestUtil.h>
#include <folly/io/IOBuf.h>
#include "eden/fs/config/ClientConfig.h"
#include "eden/fs/inodes/Dirstate.h"
#include "eden/fs/inodes/EdenDispatcher.h"
#include "eden/fs/inodes/FileData.h"
#include "eden/fs/inodes/FileHandle.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/BackingStore.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/store/hg/HgManifestImporter.h"
#include "eden/fs/testharness/FakeBackingStore.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestUtil.h"
#include "eden/fuse/MountPoint.h"

using facebook::eden::fusell::MountPoint;
using folly::ByteRange;
using folly::StringPiece;
using folly::test::TemporaryDirectory;
using folly::test::TemporaryFile;
using std::make_shared;
using std::make_unique;
using std::shared_ptr;
using std::unique_ptr;
using std::vector;

namespace facebook {
namespace eden {

bool TestMountFile::operator==(const TestMountFile& other) const {
  return path == other.path && contents == other.contents && rwx == other.rwx &&
      type == other.type;
}

TestMount::TestMount() {
  // Initialize the temporary directory.
  // This sets both testDir_, config_, localStore_, and backingStore_
  initTestDirectory();
}

TestMount::TestMount(FakeTreeBuilder& rootBuilder, bool startReady) {
  initTestDirectory();
  initialize(rootBuilder, startReady);
}

TestMount::TestMount(
    Hash initialCommitHash,
    FakeTreeBuilder& rootBuilder,
    bool startReady) {
  initTestDirectory();
  initialize(initialCommitHash, rootBuilder, startReady);
}

TestMount::~TestMount() {}

void TestMount::initialize(Hash initialCommitHash) {
  // Set the initial commit ID
  setInitialCommit(initialCommitHash);

  // Create edenMount_
  unique_ptr<ObjectStore> objectStore =
      make_unique<ObjectStore>(localStore_, backingStore_);
  edenMount_ =
      EdenMount::makeShared(std::move(config_), std::move(objectStore));
}

void TestMount::initialize(Hash commitHash, Hash rootTreeHash) {
  // Set the initial commit ID
  setInitialCommit(commitHash, rootTreeHash);

  // Create edenMount_
  unique_ptr<ObjectStore> objectStore =
      make_unique<ObjectStore>(localStore_, backingStore_);
  edenMount_ =
      EdenMount::makeShared(std::move(config_), std::move(objectStore));
}

void TestMount::initialize(
    Hash initialCommitHash,
    FakeTreeBuilder& rootBuilder,
    bool startReady) {
  // Finalize rootBuilder and get the root Tree
  rootBuilder.finalize(backingStore_, startReady);
  auto rootTree = rootBuilder.getRoot();
  // We have to make sure the root tree is ready.  The EdenMount constructor
  // blocks until it is available, so we will hang below if it isn't ready.
  rootTree->setReady();

  // Set the commit to tree mapping, and record the current commit hash
  setInitialCommit(initialCommitHash, rootTree->get().getHash());

  // Create edenMount_
  unique_ptr<ObjectStore> objectStore =
      make_unique<ObjectStore>(localStore_, backingStore_);
  edenMount_ =
      EdenMount::makeShared(std::move(config_), std::move(objectStore));
}

void TestMount::initialize(FakeTreeBuilder& rootBuilder, bool startReady) {
  initialize(makeTestHash("1"), rootBuilder, startReady);
}

void TestMount::initTestDirectory() {
  // Create the temporary directory
  testDir_ = make_unique<TemporaryDirectory>("eden_test");

  // Make the mount point and the eden client storage directories
  // inside the test directory.
  auto makedir = [](AbsolutePathPiece path) {
    ::mkdir(path.stringPiece().str().c_str(), 0755);
  };
  auto testDirPath = AbsolutePath{testDir_->path().string()};
  auto clientDirectory = testDirPath + PathComponentPiece("eden");
  makedir(clientDirectory);
  makedir(clientDirectory + PathComponentPiece("local"));
  auto mountPath = testDirPath + PathComponentPiece("mount");
  makedir(mountPath);

  // Create the ClientConfig using our newly-populated client directory
  config_ = make_unique<ClientConfig>(mountPath, clientDirectory);

  // Create localStore_ and backingStore_
  localStore_ =
      make_shared<LocalStore>(testDirPath + PathComponentPiece("rocksdb"));
  backingStore_ = make_shared<FakeBackingStore>(localStore_);
}

void TestMount::setInitialCommit(Hash commitHash) {
  // Write the commit hash to the snapshot file
  auto snapshotPath = config_->getSnapshotPath();
  folly::writeFileAtomic(
      snapshotPath.stringPiece(), commitHash.toString() + "\n");
}

void TestMount::setInitialCommit(Hash commitHash, Hash rootTreeHash) {
  // Record the commit hash to root tree hash mapping in the BackingStore
  auto* storedCommit = backingStore_->putCommit(commitHash, rootTreeHash);
  storedCommit->setReady();

  // Call setInitialCommit(hash) to write the snapshot file
  setInitialCommit(commitHash);
}

void TestMount::setInitialDirstate(
    const std::unordered_map<RelativePath, overlay::UserStatusDirective>&
        userDirectives) {
  DirstatePersistence dirstatePersistence{config_->getDirstateStoragePath()};
  dirstatePersistence.save(userDirectives);
}

void TestMount::addFile(folly::StringPiece path, std::string contents) {
  RelativePathPiece relativePath(path);
  auto treeInode = getTreeInode(relativePath.dirname());
  mode_t mode = 0644;
  int flags = 0;
  auto dispatcher = edenMount_->getDispatcher();
  auto createResult =
      dispatcher
          ->create(treeInode->getNodeId(), relativePath.basename(), mode, flags)
          .get();
  off_t off = 0;
  createResult.fh->write(contents, off);
  createResult.fh->fsync(/*datasync*/ true);
}

void TestMount::overwriteFile(folly::StringPiece path, std::string contents) {
  auto file = getFileInode(path);

  fuse_file_info info;
  info.flags = O_RDWR | O_TRUNC;
  info.fh = file->getNodeId();
  auto fileHandle = file->open(info).get();
  off_t offset = 0;
  fileHandle->write(contents, offset);
  fileHandle->fsync(/*datasync*/ true);
}

std::string TestMount::readFile(folly::StringPiece path) {
  auto file = getFileInode(path);
  auto fileData = file->getOrLoadData();
  auto attr = file->getattr().get();
  auto buf = fileData->readIntoBuffer(
      /* size */ attr.st.st_size, /* off */ 0);
  return std::string(reinterpret_cast<const char*>(buf->data()), buf->length());
}

bool TestMount::hasFileAt(folly::StringPiece path) {
  auto relativePath = RelativePathPiece{path};
  mode_t mode;
  try {
    auto child = edenMount_->getInodeBlocking(relativePath);
    mode = child->getattr().get().st.st_mode;
  } catch (const std::system_error& e) {
    if (e.code().value() == ENOENT) {
      return false;
    } else {
      throw;
    }
  }

  return S_ISREG(mode);
}

void TestMount::mkdir(folly::StringPiece path) {
  auto relativePath = RelativePathPiece{path};
  auto treeInode = getTreeInode(relativePath.dirname());
  mode_t mode = 0755;
  auto dispatcher = edenMount_->getDispatcher();
  dispatcher->mkdir(treeInode->getNodeId(), relativePath.basename(), mode)
      .get();
}

void TestMount::deleteFile(folly::StringPiece path) {
  auto relativePath = RelativePathPiece{path};
  auto treeInode = getTreeInode(relativePath.dirname());
  auto dispatcher = edenMount_->getDispatcher();
  dispatcher->unlink(treeInode->getNodeId(), relativePath.basename()).get();
}

void TestMount::rmdir(folly::StringPiece path) {
  auto relativePath = RelativePathPiece{path};
  auto treeInode = getTreeInode(relativePath.dirname());
  auto dispatcher = edenMount_->getDispatcher();
  dispatcher->rmdir(treeInode->getNodeId(), relativePath.basename()).get();
}

InodePtr TestMount::getInode(RelativePathPiece path) const {
  // Call future.get() with a timeout.  Generally in tests we expect the future
  // to be immediately ready.  We want to make sure the test does not hang
  // forever if something goes wrong.
  return edenMount_->getInode(path).get(std::chrono::milliseconds(1));
}

InodePtr TestMount::getInode(folly::StringPiece path) const {
  return getInode(RelativePathPiece{path});
}

TreeInodePtr TestMount::getTreeInode(RelativePathPiece path) const {
  return getInode(path).asTreePtr();
}

TreeInodePtr TestMount::getTreeInode(folly::StringPiece path) const {
  return getTreeInode(RelativePathPiece{path});
}

FileInodePtr TestMount::getFileInode(RelativePathPiece path) const {
  return getInode(path).asFilePtr();
}

FileInodePtr TestMount::getFileInode(folly::StringPiece path) const {
  return getFileInode(RelativePathPiece{path});
}

std::unique_ptr<Tree> TestMount::getRootTree() const {
  return edenMount_->getRootTree();
}

Dirstate* TestMount::getDirstate() const {
  return edenMount_->getDirstate();
}
}
}
