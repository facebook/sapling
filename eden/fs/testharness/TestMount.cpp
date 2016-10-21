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

#include <folly/experimental/TestUtil.h>
#include "eden/fs/config/ClientConfig.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/inodes/TreeEntryFileInode.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/BackingStore.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/NullBackingStore.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/store/hg/HgManifestImporter.h"
#include "eden/fuse/FileInode.h"
#include "eden/fuse/MountPoint.h"

using facebook::eden::fusell::MountPoint;
using folly::ByteRange;
using folly::StringPiece;
using folly::test::TemporaryDirectory;
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

  auto mountPointDir = make_unique<TemporaryDirectory>();
  shared_ptr<MountPoint> mountPoint = make_shared<MountPoint>(
      AbsolutePathPiece{mountPointDir->path().string()});

  // Note that although a TestMount is meant to be used for unit testing, we end
  // up creating an instance of a RocksDb to power the LocalStore. If this
  // becomes too expensive, we should look into creating an alternate
  // FakeLocalStore that runs completely in-memory.
  auto pathToRocksDb = make_unique<TemporaryDirectory>();
  auto localStore = make_shared<LocalStore>(
      folly::StringPiece(pathToRocksDb->path().string()));

  shared_ptr<BackingStore> backingStore = make_shared<NullBackingStore>();
  unique_ptr<ObjectStore> objectStore =
      make_unique<ObjectStore>(localStore, backingStore);

  auto overlayDir = make_unique<TemporaryDirectory>();
  shared_ptr<Overlay> overlay =
      make_shared<Overlay>(AbsolutePathPiece{overlayDir->path().string()});

  std::vector<BindMount> bindMounts;
  unique_ptr<EdenMount> edenMount = make_unique<EdenMount>(
      mountPoint, std::move(objectStore), overlay, bindMounts);

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
    localStore->putBlob(sha1, bytes, sha1);

    TreeEntry treeEntry(
        sha1, file.path.basename().stringPiece(), file.type, file.rwx);
    manifestImporter.processEntry(dirname, std::move(treeEntry));
  }
  auto rootTreeHash = manifestImporter.finish();
  auto rootTree = localStore->getTree(rootTreeHash);

  // Set the root inode as EdenServiceHandler does.
  mountPoint->setRootInode(make_shared<TreeInode>(
      edenMount.get(),
      std::move(rootTree),
      nullptr,
      FUSE_ROOT_ID,
      FUSE_ROOT_ID));

  return make_unique<TestMount>(
      std::move(edenMount),
      std::move(mountPointDir),
      std::move(pathToRocksDb),
      std::move(overlayDir));
}

void TestMount::addFile(folly::StringPiece path, std::string contents) {
  RelativePathPiece relativePath(path);
  auto treeInode = getDirInodeForPath(relativePath.dirname().stringPiece());
  mode_t modeThatSeemsToBeIgnored = 0; // TreeInode::create() uses 0600.
  int flags = 0;
  auto dispatcher = edenMount_->getMountPoint()->getDispatcher();
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
  auto dispatcher = edenMount_->getMountPoint()->getDispatcher();
  auto child =
      dispatcher
          ->lookupInodeBase(directory->getNodeId(), relativePath.basename())
          .get();
  auto file = std::dynamic_pointer_cast<TreeEntryFileInode>(child);

  fuse_file_info info;
  info.flags = O_RDWR | O_TRUNC;
  info.fh = file->getNodeId();
  auto fileHandle = file->open(info).get();
  off_t offset = 0;
  fileHandle->write(contents, offset);
  fileHandle->fsync(/*datasync*/ true);
}

void TestMount::mkdir(folly::StringPiece path) {
  auto relativePath = RelativePathPiece{path};
  auto treeInode = getDirInodeForPath(relativePath.dirname().stringPiece());
  mode_t mode = 0755;
  auto dispatcher = edenMount_->getMountPoint()->getDispatcher();
  dispatcher->mkdir(treeInode->getInode(), relativePath.basename(), mode).get();
}

void TestMount::deleteFile(folly::StringPiece path) {
  auto relativePath = RelativePathPiece{path};
  auto treeInode = getDirInodeForPath(relativePath.dirname().stringPiece());
  auto dispatcher = edenMount_->getMountPoint()->getDispatcher();
  dispatcher->unlink(treeInode->getInode(), relativePath.basename()).get();
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

std::shared_ptr<TreeEntryFileInode> TestMount::getFileInodeForPath(
    folly::StringPiece path) const {
  auto file =
      edenMount_->getMountPoint()->getInodeBaseForPath(RelativePathPiece{path});
  auto fileTreeEntry = std::dynamic_pointer_cast<TreeEntryFileInode>(file);
  if (fileTreeEntry != nullptr) {
    return fileTreeEntry;
  } else {
    throw std::runtime_error(
        folly::to<std::string>("Could not cast to TreeEntryFileInode: ", path));
  }
}

std::unique_ptr<Tree> TestMount::getRootTree() const {
  auto root = getDirInodeForPath("");
  auto dirTreeEntry = std::dynamic_pointer_cast<TreeInode>(root);
  DCHECK(dirTreeEntry != nullptr);
  {
    auto dir = dirTreeEntry->getContents().rlock();
    auto& rootTreeHash = dir->treeHash.value();
    auto tree = edenMount_->getObjectStore()->getTree(rootTreeHash);
    return tree;
  }
}
}
}
