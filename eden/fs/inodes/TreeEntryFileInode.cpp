/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "TreeEntryFileInode.h"

#include "FileData.h"
#include "TreeEntryFileHandle.h"
#include "eden/fs/overlay/Overlay.h"
#include "eden/fs/store/LocalStore.h"

using folly::Future;
using folly::StringPiece;
using folly::checkUnixError;
using std::string;
using std::vector;

const string kXattrSha1 = "user.sha1";

namespace facebook {
namespace eden {

TreeEntryFileInode::TreeEntryFileInode(
    fuse_ino_t ino,
    std::shared_ptr<TreeInode> parentInode,
    const TreeEntry* entry)
    : fusell::FileInode(ino), parentInode_(parentInode), entry_(entry) {}

folly::Future<fusell::Dispatcher::Attr> TreeEntryFileInode::getattr() {
  auto data = getOrLoadData();

  // Future optimization opportunity: right now, if we have not already
  // materialized the data from the entry_, we have to materialize it
  // from the store.  If we augmented our metadata we could avoid this,
  // and this would speed up operations like `ls`.
  data->materialize(
      O_RDONLY,
      fusell::InodeNameManager::get()->resolvePathToNode(getNodeId()));

  fusell::Dispatcher::Attr attr;
  attr.st = data->stat();
  attr.st.st_ino = ino_;
  return attr;
}

folly::Future<std::string> TreeEntryFileInode::readlink() {
  std::unique_lock<std::mutex> lock(mutex_);

  if (!entry_) {
    struct stat st;
    auto localPath = getLocalPath();

    // Figure out how much space we need to hold the symlink target.
    checkUnixError(lstat(localPath.c_str(), &st));

    // Allocate a string of the appropriate size.
    std::string buf;
    buf.resize(st.st_size, 0 /* filled with zeroes */);

    // Read the link into the string buffer.
    auto res = ::readlink(
        localPath.c_str(), &buf[0], buf.size() + 1 /* for nul terminator */);
    checkUnixError(res);
    CHECK_EQ(st.st_size, res) << "symlink size TOCTOU";

    return buf;
  }

  switch (entry_->getFileType()) {
    case FileType::SYMLINK: {
      auto blob = parentInode_->getStore()->getBlob(entry_->getHash());
      auto buf = blob->getContents();
      return buf.moveToFbString().toStdString();
    }

    default:
      // man 2 readlink says:  EINVAL The named file is not a symbolic link.
      folly::throwSystemErrorExplicit(EINVAL);
  }
}

std::shared_ptr<FileData> TreeEntryFileInode::getOrLoadData() {
  std::unique_lock<std::mutex> lock(mutex_);
  if (!data_) {
    data_ = std::make_shared<FileData>(
        mutex_, parentInode_->getStore(), parentInode_->getOverlay(), entry_);
  }

  return data_;
}

void TreeEntryFileInode::fileHandleDidClose() {
  std::unique_lock<std::mutex> lock(mutex_);
  if (data_.unique()) {
    // We're the only remaining user, no need to keep it around
    data_.reset();
  }
}

AbsolutePath TreeEntryFileInode::getLocalPath() const {
  return parentInode_->getOverlay()->getLocalDir() +
      fusell::InodeNameManager::get()->resolvePathToNode(getNodeId());
}

folly::Future<std::unique_ptr<fusell::FileHandle>> TreeEntryFileInode::open(
    const struct fuse_file_info& fi) {
  auto data = getOrLoadData();
  SCOPE_EXIT {
    data.reset();
    fileHandleDidClose();
  };
  data->materialize(
      fi.flags,
      fusell::InodeNameManager::get()->resolvePathToNode(getNodeId()));

  return std::make_unique<TreeEntryFileHandle>(
      std::static_pointer_cast<TreeEntryFileInode>(shared_from_this()),
      data,
      fi.flags);
}

Future<vector<string>> TreeEntryFileInode::listxattr() {
  // Currently, we only return a non-empty vector for regular files, and we
  // assume that the SHA-1 is present without checking the ObjectStore.
  vector<string> attributes;
  if (!entry_ || entry_->getFileType() != FileType::REGULAR_FILE) {
    return attributes;
  }

  attributes.emplace_back(kXattrSha1);
  return attributes;
}

Future<string> TreeEntryFileInode::getxattr(StringPiece name) {
  // Currently, we only support the xattr for the SHA-1 of a regular file.
  if (!entry_ || name != kXattrSha1 ||
      entry_->getFileType() != FileType::REGULAR_FILE) {
    return "";
  }

  // TODO(mbolin): Make this more fault-tolerant. Currently, there is no logic
  // to account for the case where we don't have the SHA-1 for the blob, the
  // hash doesn't correspond to a blob, etc.
  auto sha1 = parentInode_->getStore()->getSha1ForBlob(entry_->getHash());
  return sha1->toString();
}

const TreeEntry* TreeEntryFileInode::getEntry() const {
  return entry_;
}
}
}
