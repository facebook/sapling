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
#include <sys/types.h>
#include "eden/fs/config/ClientConfig.h"
#include "eden/fs/inodes/EdenDispatcher.h"
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

using folly::ByteRange;
using folly::Future;
using folly::StringPiece;
using folly::Unit;
using folly::makeFuture;
using folly::test::TemporaryDirectory;
using folly::test::TemporaryFile;
using std::make_shared;
using std::make_unique;
using std::shared_ptr;
using std::string;
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

void TestMount::initialize(
    Hash initialCommitHash,
    std::chrono::system_clock::time_point lastCheckoutTime) {
  // Set the initial commit ID
  setInitialCommit(initialCommitHash);

  // Create edenMount_
  unique_ptr<ObjectStore> objectStore =
      make_unique<ObjectStore>(localStore_, backingStore_);
  edenMount_ = EdenMount::create(
                   std::move(config_),
                   std::move(objectStore),
                   AbsolutePathPiece(),
                   &stats_)
                   .get();
  edenMount_->setLastCheckoutTime(lastCheckoutTime);
}

void TestMount::initialize(Hash commitHash, Hash rootTreeHash) {
  // Set the initial commit ID
  setInitialCommit(commitHash, rootTreeHash);

  // Create edenMount_
  unique_ptr<ObjectStore> objectStore =
      make_unique<ObjectStore>(localStore_, backingStore_);
  edenMount_ = EdenMount::create(
                   std::move(config_),
                   std::move(objectStore),
                   AbsolutePathPiece(),
                   &stats_)
                   .get();
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
  edenMount_ = EdenMount::create(
                   std::move(config_),
                   std::move(objectStore),
                   AbsolutePathPiece(),
                   &stats_)
                   .get();
}

Hash TestMount::nextCommitHash() {
  auto number = commitNumber_.fetch_add(1);
  return makeTestHash(folly::to<string>(number));
}

void TestMount::initialize(FakeTreeBuilder& rootBuilder, bool startReady) {
  initialize(nextCommitHash(), rootBuilder, startReady);
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

void TestMount::remount() {
  // Create a new copy of the ClientConfig
  auto config = make_unique<ClientConfig>(*edenMount_->getConfig());
  // Create a new ObjectStore pointing to our local store and backing storpe
  unique_ptr<ObjectStore> objectStore =
      make_unique<ObjectStore>(localStore_, backingStore_);

  // Reset the edenMount_ pointer.  This will destroy the old EdenMount
  // assuming that no-one else still has any references to it.
  //
  // We do this explicitly so that the old edenMount_ is destroyed before we
  // create the new one below.
  edenMount_.reset();

  // Create a new EdenMount object.
  edenMount_ = EdenMount::create(
                   std::move(config),
                   std::move(objectStore),
                   AbsolutePathPiece(),
                   &stats_)
                   .get();
}

void TestMount::resetCommit(FakeTreeBuilder& builder, bool setReady) {
  resetCommit(nextCommitHash(), builder, setReady);
}

void TestMount::resetCommit(
    Hash commitHash,
    FakeTreeBuilder& builder,
    bool setReady) {
  auto* rootTree = builder.finalize(backingStore_, setReady);
  auto* storedCommit =
      backingStore_->putCommit(commitHash, rootTree->get().getHash());
  storedCommit->setReady();

  // The root tree needs to be made ready too, even if setReady is false.
  // resetCommit() won't return until until the root tree can be loaded.
  if (!setReady) {
    rootTree->setReady();
  }

  edenMount_->resetParent(commitHash);
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

void TestMount::addFile(folly::StringPiece path, folly::StringPiece contents) {
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
  auto attr = file->getattr().get();
  auto buf = file->readIntoBuffer(
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

void TestMount::chmod(folly::StringPiece path, mode_t permissions) {
  auto inode = getInode(RelativePathPiece{path});

  struct stat desiredAttr;
  memset(&desiredAttr, 0, sizeof(desiredAttr));
  desiredAttr.st_mode = permissions;
  inode->setattr(desiredAttr, FUSE_SET_ATTR_MODE).get();
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

void TestMount::loadAllInodes() {
  loadAllInodesFuture().get();
}

Future<Unit> TestMount::loadAllInodesFuture() {
  return loadAllInodesFuture(edenMount_->getRootInode());
}

void TestMount::loadAllInodes(const TreeInodePtr& treeInode) {
  loadAllInodesFuture(treeInode).get();
}

Future<Unit> TestMount::loadAllInodesFuture(const TreeInodePtr& treeInode) {
  // Build a list of child names to load.
  // (If necessary we could make a more efficient version of this that starts
  // all the child loads while holding the lock.  However, we don't really care
  // about efficiency for test code, and this is much simpler.)
  std::vector<PathComponent> childNames;
  {
    auto contents = treeInode->getContents().rlock();
    for (const auto& entry : contents->entries) {
      childNames.emplace_back(entry.first);
    }
  }

  // Now start all the loads.
  std::vector<Future<Unit>> childFutures;
  for (const auto& name : childNames) {
    auto childFuture = treeInode->getOrLoadChild(name).then([](InodePtr child) {
      TreeInodePtr childTree = child.asTreePtrOrNull();
      if (childTree) {
        return loadAllInodesFuture(childTree);
      }
      return makeFuture();
    });
    childFutures.emplace_back(std::move(childFuture));
  }
  return folly::collect(childFutures).unit();
}

std::shared_ptr<const Tree> TestMount::getRootTree() const {
  return edenMount_->getRootTree();
}

} // namespace eden
} // namespace facebook
