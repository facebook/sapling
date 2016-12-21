/*
 *  Copyright (c) 2016, Facebook, Inc.
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

TestMount::TestMount(
    std::shared_ptr<EdenMount> edenMount,
    std::unique_ptr<folly::test::TemporaryDirectory> testDir)
    : testDir_(std::move(testDir)), edenMount_(std::move(edenMount)) {}

TestMount::~TestMount() {}

BaseTestMountBuilder::BaseTestMountBuilder() {
  // Initialize the TestMount's temporary directory.
  // This sets both testDir_, config_, localStore_, and backingStore_
  initTestDirectory();
}

BaseTestMountBuilder::~BaseTestMountBuilder() {}

unique_ptr<TestMount> BaseTestMountBuilder::build() {
  // Invoke populateStore() so subclasses can populate the stores, if needed.
  populateStore();

  // Now create the EdenMount
  unique_ptr<ObjectStore> objectStore =
      make_unique<ObjectStore>(localStore_, backingStore_);
  auto edenMount =
      std::make_shared<EdenMount>(std::move(config_), std::move(objectStore));
  return make_unique<TestMount>(std::move(edenMount), std::move(testDir_));
}

void BaseTestMountBuilder::initTestDirectory() {
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

void BaseTestMountBuilder::setCommit(Hash commitHash, Hash rootTreeHash) {
  // Record the commit hash to root tree hash mapping in the BackingStore
  auto* storedCommit = backingStore_->putCommit(commitHash, rootTreeHash);
  storedCommit->setReady();

  // Write the commit hash to the snapshot file
  writeSnapshotFile(commitHash);
}

void BaseTestMountBuilder::writeSnapshotFile(Hash commitHash) {
  auto snapshotPath = config_->getSnapshotPath();
  folly::writeFileAtomic(
      snapshotPath.stringPiece(), commitHash.toString() + "\n");
}

void BaseTestMountBuilder::populateStore() {
  // Base class implementation is a no-op
}

TestMountBuilder::TestMountBuilder() {}

TestMountBuilder::~TestMountBuilder() {}

void TestMountBuilder::populateStore() {
  std::sort(
      files_.begin(),
      files_.end(),
      [](const TestMountFile& a, const TestMountFile& b) {
        return a.path < b.path;
      });
  // Make sure there are no two items with the same path.
  if (files_.size() > 1) {
    for (auto it = files_.begin() + 1; it != files_.end(); ++it) {
      auto prev = it - 1;
      if (prev->path == it->path) {
        throw std::runtime_error(folly::to<std::string>(
            "Duplicate path added to TestMountBuilder: ", it->path));
      }
    }
  }

  // Use HgManifestImporter to create the appropriate intermediate Tree objects
  // for the set of files that the user specified with proper hashes.
  HgManifestImporter manifestImporter(getLocalStore().get());
  for (auto& file : files_) {
    auto dirname = file.path.dirname();

    // For simplicity, we use the SHA-1 of the contents as the Hash id of the
    // Blob. Note this differs from Git where the id of a Blob is the SHA-1 of a
    // header plus the contents.
    auto bytes = ByteRange(StringPiece(file.contents));
    auto sha1 = Hash::sha1(bytes);

    Hash dummyHash;
    auto buf = folly::IOBuf::copyBuffer(file.contents);
    auto blobWithDummyHash = Blob(dummyHash, *buf);
    // There's a few issues here:
    // 1. Apparently putBlob() does not look at blob.getHash(). It seems
    //    dangerous that we can construct a Blob with an arbitrary id that has
    //    nothing to do with its contents.
    // 2. putBlob() computes the SHA-1 of the blob's contents, which duplicates
    //    what we have done above.
    // 3. We cannot easily use the 3-arg form of putBlob() because it takes raw
    //    blobData instead of inserting the required header as the two-arg form
    //    of putBlob() does. The way the header insertion logic, as written, is
    //    not easy to extract out.
    getLocalStore()->putBlob(sha1, &blobWithDummyHash);

    TreeEntry treeEntry(
        sha1, file.path.basename().stringPiece(), file.type, file.rwx);
    manifestImporter.processEntry(dirname, std::move(treeEntry));
  }
  auto rootTreeHash = manifestImporter.finish();

  // If we have user directives, put them in the dirstate file
  if (!userDirectives_.empty()) {
    DirstatePersistence dirstatePersistence{
        getConfig()->getDirstateStoragePath()};
    dirstatePersistence.save(userDirectives_);
  }

  // Pick an arbitrary commit ID, and store that it maps to the root tree that
  // HgManifestImporter built.
  auto commitHash = makeTestHash("cccc");
  setCommit(commitHash, rootTreeHash);
}

void TestMountBuilder::addUserDirectives(
    const std::unordered_map<RelativePath, overlay::UserStatusDirective>&
        userDirectives) {
  userDirectives_.insert(userDirectives.begin(), userDirectives.end());
}

void TestMount::addFile(folly::StringPiece path, std::string contents) {
  RelativePathPiece relativePath(path);
  auto treeInode = getTreeInode(relativePath.dirname());
  mode_t modeThatSeemsToBeIgnored = 0; // TreeInode::create() uses 0600.
  int flags = 0;
  auto dispatcher = edenMount_->getDispatcher();
  auto createResult = dispatcher
                          ->create(
                              treeInode->getInode(),
                              relativePath.basename(),
                              modeThatSeemsToBeIgnored,
                              flags)
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
    auto child = edenMount_->getInodeBase(relativePath);
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
  dispatcher->mkdir(treeInode->getInode(), relativePath.basename(), mode).get();
}

void TestMount::deleteFile(folly::StringPiece path) {
  auto relativePath = RelativePathPiece{path};
  auto treeInode = getTreeInode(relativePath.dirname());
  auto dispatcher = edenMount_->getDispatcher();
  dispatcher->unlink(treeInode->getInode(), relativePath.basename()).get();
}

void TestMount::rmdir(folly::StringPiece path) {
  auto relativePath = RelativePathPiece{path};
  auto treeInode = getTreeInode(relativePath.dirname());
  auto dispatcher = edenMount_->getDispatcher();
  dispatcher->rmdir(treeInode->getInode(), relativePath.basename()).get();
}

TreeInodePtr TestMount::getTreeInode(RelativePathPiece path) const {
  return edenMount_->getTreeInode(path);
}

TreeInodePtr TestMount::getTreeInode(folly::StringPiece path) const {
  return edenMount_->getTreeInode(RelativePathPiece{path});
}

FileInodePtr TestMount::getFileInode(RelativePathPiece path) const {
  return edenMount_->getFileInode(path);
}

FileInodePtr TestMount::getFileInode(folly::StringPiece path) const {
  return edenMount_->getFileInode(RelativePathPiece{path});
}

std::unique_ptr<Tree> TestMount::getRootTree() const {
  return edenMount_->getRootTree();
}

Dirstate* TestMount::getDirstate() const {
  return edenMount_->getDirstate();
}
}
}
