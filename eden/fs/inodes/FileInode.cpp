/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "FileInode.h"

#include "EdenMount.h"
#include "FileData.h"
#include "FileHandle.h"
#include "InodeError.h"
#include "Overlay.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/utils/XAttr.h"

using folly::Future;
using folly::StringPiece;
using folly::checkUnixError;
using std::string;
using std::vector;

namespace facebook {
namespace eden {

FileInode::FileInode(
    fuse_ino_t ino,
    TreeInodePtr parentInode,
    PathComponentPiece name,
    TreeInode::Entry* entry)
    : InodeBase(ino, parentInode, name),
      parentInode_(parentInode),
      entry_(entry),
      data_(std::make_shared<FileData>(this, mutex_, entry)) {}

FileInode::FileInode(
    fuse_ino_t ino,
    TreeInodePtr parentInode,
    PathComponentPiece name,
    TreeInode::Entry* entry,
    folly::File&& file)
    : InodeBase(ino, parentInode, name),
      parentInode_(parentInode),
      entry_(entry),
      data_(std::make_shared<FileData>(this, mutex_, entry_, std::move(file))) {
}

folly::Future<fusell::Dispatcher::Attr> FileInode::getattr() {
  auto data = getOrLoadData();
  auto path = getPathBuggy();

  // Future optimization opportunity: right now, if we have not already
  // materialized the data from the entry_, we have to materialize it
  // from the store.  If we augmented our metadata we could avoid this,
  // and this would speed up operations like `ls`.
  auto overlay = parentInode_->getOverlay();
  data->materializeForRead(O_RDONLY, path, overlay);

  fusell::Dispatcher::Attr attr(parentInode_->getMount()->getMountPoint());
  attr.st = data->stat();
  attr.st.st_ino = getNodeId();
  return attr;
}

folly::Future<fusell::Dispatcher::Attr> FileInode::setattr(
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

  auto path = getPathBuggy();
  auto overlay = parentInode_->getOverlay();
  data->materializeForWrite(open_flags, path, overlay);

  fusell::Dispatcher::Attr result(parentInode_->getMount()->getMountPoint());
  result.st = data->setAttr(attr, to_set);
  result.st.st_ino = getNodeId();

  parentInode_->getMount()->getJournal().wlock()->addDelta(
      std::make_unique<JournalDelta>(JournalDelta{path}));

  return result;
}

folly::Future<std::string> FileInode::readlink() {
  std::unique_lock<std::mutex> lock(mutex_);

  DCHECK_NOTNULL(entry_);
  if (!S_ISLNK(entry_->mode)) {
    // man 2 readlink says:  EINVAL The named file is not a symbolic link.
    throw InodeError(EINVAL, inodePtrFromThis(), "not a symlink");
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

std::shared_ptr<FileData> FileInode::getOrLoadData() {
  std::unique_lock<std::mutex> lock(mutex_);
  if (!data_) {
    data_ = std::make_shared<FileData>(this, mutex_, entry_);
  }

  return data_;
}

void FileInode::fileHandleDidClose() {
  std::unique_lock<std::mutex> lock(mutex_);
  if (data_.unique()) {
    // We're the only remaining user, no need to keep it around
    data_.reset();
  }
}

AbsolutePath FileInode::getLocalPath() const {
  return parentInode_->getOverlay()->getContentDir() + getPathBuggy();
}

folly::Future<std::shared_ptr<fusell::FileHandle>> FileInode::open(
    const struct fuse_file_info& fi) {
  auto data = getOrLoadData();
  SCOPE_EXIT {
    data.reset();
    fileHandleDidClose();
  };
  auto overlay = parentInode_->getOverlay();
  if (fi.flags & (O_RDWR | O_WRONLY | O_CREAT | O_TRUNC)) {
    parentInode_->materializeDirAndParents();
    data->materializeForWrite(fi.flags, getPathBuggy(), overlay);
  } else {
    data->materializeForRead(fi.flags, getPathBuggy(), overlay);
  }

  return std::make_shared<FileHandle>(
      std::static_pointer_cast<FileInode>(shared_from_this()), data, fi.flags);
}

std::shared_ptr<FileHandle> FileInode::finishCreate() {
  auto data = getOrLoadData();
  SCOPE_EXIT {
    data.reset();
    fileHandleDidClose();
  };
  data->materializeForWrite(0, getPathBuggy(), parentInode_->getOverlay());

  return std::make_shared<FileHandle>(
      std::static_pointer_cast<FileInode>(shared_from_this()), data, 0);
}

Future<vector<string>> FileInode::listxattr() {
  // Currently, we only return a non-empty vector for regular files, and we
  // assume that the SHA-1 is present without checking the ObjectStore.
  vector<string> attributes;

  if (S_ISREG(entry_->mode)) {
    attributes.emplace_back(kXattrSha1.str());
  }
  return attributes;
}

Future<string> FileInode::getxattr(StringPiece name) {
  // Currently, we only support the xattr for the SHA-1 of a regular file.
  if (name != kXattrSha1) {
    throw InodeError(kENOATTR, inodePtrFromThis());
  }

  return getSHA1().get().toString();
}

Future<Hash> FileInode::getSHA1() {
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
    throw InodeError(kENOATTR, inodePtrFromThis());
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
  return parentInode_->getStore()->getSha1ForBlob(entry_->hash.value());
}

const TreeInode::Entry* FileInode::getEntry() const {
  return entry_;
}
}
}
