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
#include "eden/fs/inodes/FileData.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/BackingStore.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/store/hg/HgManifestImporter.h"
#include "eden/fs/testharness/TestBackingStore.h"
#include "eden/fuse/FileInode.h"
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

unique_ptr<TestMount> TestMountBuilder::build() {
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

  auto testDir = make_unique<TemporaryDirectory>("eden_test");
  AbsolutePath testDirPath{testDir->path().string()};

  auto pathToRocksDb = testDirPath + PathComponentPiece("rocksdb");
  auto localStore = make_shared<LocalStore>(pathToRocksDb.stringPiece());
  shared_ptr<BackingStore> backingStore =
      make_shared<TestBackingStore>(localStore);
  unique_ptr<ObjectStore> objectStore =
      make_unique<ObjectStore>(localStore, backingStore);

  // Use HgManifestImporter to create the appropriate intermediate Tree objects
  // for the set of files that the user specified with proper hashes.
  HgManifestImporter manifestImporter(localStore.get());
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
    localStore->putBlob(sha1, &blobWithDummyHash);

    TreeEntry treeEntry(
        sha1, file.path.basename().stringPiece(), file.type, file.rwx);
    manifestImporter.processEntry(dirname, std::move(treeEntry));
  }
  auto rootTreeHash = manifestImporter.finish();

  // Create the ClientConfig for the test mount
  auto config = setupClientConfig(testDirPath, rootTreeHash);

  // If we have user directives, put them in the dirstate file
  if (!userDirectives_.empty()) {
    DirstatePersistence dirstatePersistence{config->getDirstateStoragePath()};
    dirstatePersistence.save(userDirectives_);
  }

  auto edenMount =
      std::make_shared<EdenMount>(std::move(config), std::move(objectStore));
  return make_unique<TestMount>(std::move(edenMount), std::move(testDir));
}

void TestMountBuilder::addUserDirectives(
    std::unordered_map<RelativePath, overlay::UserStatusDirective>&&
        userDirectives) {
  for (auto& pair : userDirectives) {
    userDirectives_.emplace(pair.first, pair.second);
  }
}

std::unique_ptr<ClientConfig> TestMountBuilder::setupClientConfig(
    AbsolutePathPiece testDirectory,
    Hash rootTreeHash) {
  // Make the mount point and the eden client storage directories
  // inside the test directory.
  auto makedir = [](AbsolutePathPiece path) {
    ::mkdir(path.stringPiece().str().c_str(), 0755);
  };
  AbsolutePath clientDirectory = testDirectory + PathComponentPiece("eden");
  makedir(clientDirectory);
  makedir(clientDirectory + PathComponentPiece("local"));
  AbsolutePath mountPath = testDirectory + PathComponentPiece("mount");
  makedir(mountPath);

  // Write the root hash to the snapshot file
  auto snapshotPath = clientDirectory + PathComponentPiece{"SNAPSHOT"};
  folly::writeFileAtomic(
      snapshotPath.stringPiece(), rootTreeHash.toString() + "\n");

  // Return a ClientConfig using our newly-populated client directory
  return make_unique<ClientConfig>(mountPath, clientDirectory);
}

void TestMount::addFile(folly::StringPiece path, std::string contents) {
  RelativePathPiece relativePath(path);
  auto treeInode = getDirInodeForPath(relativePath.dirname().stringPiece());
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
  auto relativePath = RelativePathPiece{path};
  auto directory =
      edenMount_->getMountPoint()->getDirInodeForPath(relativePath.dirname());
  auto dispatcher = edenMount_->getDispatcher();
  auto file = getFileInodeForPath(path);

  fuse_file_info info;
  info.flags = O_RDWR | O_TRUNC;
  info.fh = file->getNodeId();
  auto fileHandle = file->open(info).get();
  off_t offset = 0;
  fileHandle->write(contents, offset);
  fileHandle->fsync(/*datasync*/ true);
}

std::string TestMount::readFile(folly::StringPiece path) {
  auto relativePath = RelativePathPiece{path};
  auto directory =
      edenMount_->getMountPoint()->getDirInodeForPath(relativePath.dirname());
  auto file = getFileInodeForPath(path);
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
    auto child = edenMount_->getMountPoint()->getInodeBaseForPath(relativePath);
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
  auto treeInode = getDirInodeForPath(relativePath.dirname().stringPiece());
  mode_t mode = 0755;
  auto dispatcher = edenMount_->getDispatcher();
  dispatcher->mkdir(treeInode->getInode(), relativePath.basename(), mode).get();
}

void TestMount::deleteFile(folly::StringPiece path) {
  auto relativePath = RelativePathPiece{path};
  auto treeInode = getDirInodeForPath(relativePath.dirname().stringPiece());
  auto dispatcher = edenMount_->getDispatcher();
  dispatcher->unlink(treeInode->getInode(), relativePath.basename()).get();
}

void TestMount::rmdir(folly::StringPiece path) {
  auto relativePath = RelativePathPiece{path};
  auto treeInode = getDirInodeForPath(relativePath.dirname().stringPiece());
  auto dispatcher = edenMount_->getDispatcher();
  dispatcher->rmdir(treeInode->getInode(), relativePath.basename()).get();
}

std::shared_ptr<TreeInode> TestMount::getDirInodeForPath(
    folly::StringPiece path) const {
  auto directory =
      edenMount_->getMountPoint()->getInodeBaseForPath(RelativePathPiece{path});
  auto treeInode = std::dynamic_pointer_cast<TreeInode>(directory);
  if (treeInode != nullptr) {
    return treeInode;
  } else {
    throw std::runtime_error(
        folly::to<std::string>("Could not cast to TreeInode: ", path));
  }
}

std::shared_ptr<FileInode> TestMount::getFileInodeForPath(
    folly::StringPiece path) const {
  auto file =
      edenMount_->getMountPoint()->getInodeBaseForPath(RelativePathPiece{path});
  auto fileTreeEntry = std::dynamic_pointer_cast<FileInode>(file);
  if (fileTreeEntry != nullptr) {
    return fileTreeEntry;
  } else {
    throw std::runtime_error(
        folly::to<std::string>("Could not cast to FileInode: ", path));
  }
}

std::unique_ptr<Tree> TestMount::getRootTree() const {
  return edenMount_->getRootTree();
}

Dirstate* TestMount::getDirstate() const {
  return edenMount_->getDirstate();
}
}
}
