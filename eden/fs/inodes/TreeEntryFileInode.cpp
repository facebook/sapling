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
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/overlay/Overlay.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/utils/XAttr.h"

using folly::Future;
using folly::StringPiece;
using folly::checkUnixError;
using std::string;
using std::vector;

namespace facebook {
namespace eden {

TreeEntryFileInode::TreeEntryFileInode(
    fuse_ino_t ino,
    std::shared_ptr<TreeInode> parentInode,
    TreeInode::Entry* entry)
    : fusell::FileInode(ino),
      parentInode_(parentInode),
      entry_(entry),
      data_(
          std::make_shared<FileData>(mutex_, parentInode_->getMount(), entry)) {
}

TreeEntryFileInode::TreeEntryFileInode(
    fuse_ino_t ino,
    std::shared_ptr<TreeInode> parentInode,
    TreeInode::Entry* entry,
    folly::File&& file)
    : fusell::FileInode(ino),
      parentInode_(parentInode),
      entry_(entry),
      data_(std::make_shared<FileData>(
          mutex_,
          parentInode_->getMount(),
          entry_,
          std::move(file))) {}

folly::Future<fusell::Dispatcher::Attr> TreeEntryFileInode::getattr() {
  auto data = getOrLoadData();
  auto path = parentInode_->getNameMgr()->resolvePathToNode(getNodeId());

  // Future optimization opportunity: right now, if we have not already
  // materialized the data from the entry_, we have to materialize it
  // from the store.  If we augmented our metadata we could avoid this,
  // and this would speed up operations like `ls`.
  auto overlay = parentInode_->getOverlay();
  data->materializeForRead(O_RDONLY, path, overlay);

  fusell::Dispatcher::Attr attr;
  attr.st = data->stat();
  attr.st.st_ino = getNodeId();
  return attr;
}

folly::Future<fusell::Dispatcher::Attr> TreeEntryFileInode::setattr(
    const struct stat& attr,
    int to_set) {
  auto data = getOrLoadData();
  int open_flags = O_RDWR;

  // Minor optimization: if we know that the file is being completed truncated
  // as part of this operation, there's no need to fetch the underlying data,
  // so pass on the truncate flag our underlying open call
  if ((to_set & FUSE_SET_ATTR_SIZE) && attr.st_size == 0) {
    open_flags |= O_TRUNC;
  }

  parentInode_->materializeDirAndParents();

  auto path = parentInode_->getNameMgr()->resolvePathToNode(getNodeId());
  auto overlay = parentInode_->getOverlay();
  data->materializeForWrite(open_flags, path, overlay);

  fusell::Dispatcher::Attr result;
  result.st = data->setAttr(attr, to_set);
  result.st.st_ino = getNodeId();
  return result;
}

folly::Future<std::string> TreeEntryFileInode::readlink() {
  std::unique_lock<std::mutex> lock(mutex_);

  DCHECK_NOTNULL(entry_);
  if (!S_ISLNK(entry_->mode)) {
    // man 2 readlink says:  EINVAL The named file is not a symbolic link.
    folly::throwSystemErrorExplicit(EINVAL);
  }

  if (entry_->materialized) {
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

  // Load the symlink contents from the store
  auto blob = parentInode_->getStore()->getBlob(entry_->hash.value());
  auto buf = blob->getContents();
  return buf.moveToFbString().toStdString();
}

std::shared_ptr<FileData> TreeEntryFileInode::getOrLoadData() {
  std::unique_lock<std::mutex> lock(mutex_);
  if (!data_) {
    data_ =
        std::make_shared<FileData>(mutex_, parentInode_->getMount(), entry_);
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
  return parentInode_->getOverlay()->getContentDir() +
      parentInode_->getNameMgr()->resolvePathToNode(getNodeId());
}

folly::Future<std::shared_ptr<fusell::FileHandle>> TreeEntryFileInode::open(
    const struct fuse_file_info& fi) {
  auto data = getOrLoadData();
  SCOPE_EXIT {
    data.reset();
    fileHandleDidClose();
  };
  auto overlay = parentInode_->getOverlay();
  if (fi.flags & (O_RDWR | O_WRONLY | O_CREAT | O_TRUNC)) {
    parentInode_->materializeDirAndParents();
    data->materializeForWrite(
        fi.flags,
        parentInode_->getNameMgr()->resolvePathToNode(getNodeId()),
        overlay);
  } else {
    data->materializeForRead(
        fi.flags,
        parentInode_->getNameMgr()->resolvePathToNode(getNodeId()),
        overlay);
  }

  return std::make_shared<TreeEntryFileHandle>(
      std::static_pointer_cast<TreeEntryFileInode>(shared_from_this()),
      data,
      fi.flags);
}

std::shared_ptr<fusell::FileHandle> TreeEntryFileInode::finishCreate() {
  auto data = getOrLoadData();
  SCOPE_EXIT {
    data.reset();
    fileHandleDidClose();
  };
  data->materializeForWrite(
      0,
      parentInode_->getNameMgr()->resolvePathToNode(getNodeId()),
      parentInode_->getOverlay());

  return std::make_shared<TreeEntryFileHandle>(
      std::static_pointer_cast<TreeEntryFileInode>(shared_from_this()),
      data,
      0);
}

Future<vector<string>> TreeEntryFileInode::listxattr() {
  // Currently, we only return a non-empty vector for regular files, and we
  // assume that the SHA-1 is present without checking the ObjectStore.
  vector<string> attributes;

  if (S_ISREG(entry_->mode)) {
    attributes.emplace_back(kXattrSha1.str());
  }
  return attributes;
}

Future<string> TreeEntryFileInode::getxattr(StringPiece name) {
  // Currently, we only support the xattr for the SHA-1 of a regular file.
  if (name != kXattrSha1) {
    folly::throwSystemErrorExplicit(kENOATTR);
  }

  return getSHA1().get().toString();
}

Future<Hash> TreeEntryFileInode::getSHA1() {
  // Some ugly looking stuff to avoid materializing the file if we haven't
  // done so already.
  std::unique_lock<std::mutex> lock(mutex_);
  if (data_) {
    // We already have context, ask it to supply the results.
    return data_->getSha1Locked(lock);
  }
  CHECK_NOTNULL(entry_);

  if (!S_ISREG(entry_->mode)) {
    // We only define a SHA-1 value for regular files
    folly::throwSystemErrorExplicit(kENOATTR);
  }

  if (entry_->materialized) {
    // The O_NOFOLLOW here prevents us from attempting to read attributes
    // from a symlink.
    auto filePath = getLocalPath();
    folly::File file(filePath.c_str(), O_RDONLY | O_NOFOLLOW);

    // Return the property from the existing file.
    // If it isn't set it means that someone was poking into the overlay and
    // we'll return the standard kENOATTR back to the caller in that case.
    return Hash(fgetxattr(file.fd(), kXattrSha1));
  }

  // TODO(mbolin): Make this more fault-tolerant. Currently, there is no logic
  // to account for the case where we don't have the SHA-1 for the blob, the
  // hash doesn't correspond to a blob, etc.
  return *parentInode_->getStore()->getSha1ForBlob(entry_->hash.value()).get();
}

const TreeInode::Entry* TreeEntryFileInode::getEntry() const {
  return entry_;
}
}
}
