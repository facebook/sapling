/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/inodes/FileInode.h"

#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileData.h"
#include "eden/fs/inodes/FileHandle.h"
#include "eden/fs/inodes/InodeError.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/store/BlobMetadata.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/utils/XAttr.h"

using folly::checkUnixError;
using folly::Future;
using folly::makeFuture;
using folly::StringPiece;
using std::shared_ptr;
using std::string;
using std::vector;

namespace facebook {
namespace eden {

FileInode::State::State(
    FileInode* inode,
    mode_t m,
    const folly::Optional<Hash>& h)
    : data(std::make_shared<FileData>(inode, h)),
      mode(m),
      creationTime(std::chrono::system_clock::now()),
      hash(h) {}

FileInode::State::State(
    FileInode* inode,
    mode_t m,
    folly::File&& file,
    dev_t rdev)
    : data(std::make_shared<FileData>(inode, std::move(file))),
      mode(m),
      rdev(rdev),
      creationTime(std::chrono::system_clock::now()) {}

FileInode::FileInode(
    fuse_ino_t ino,
    TreeInodePtr parentInode,
    PathComponentPiece name,
    mode_t mode,
    const folly::Optional<Hash>& hash)
    : InodeBase(ino, std::move(parentInode), name),
      state_(folly::in_place, this, mode, hash) {}

FileInode::FileInode(
    fuse_ino_t ino,
    TreeInodePtr parentInode,
    PathComponentPiece name,
    mode_t mode,
    folly::File&& file,
    dev_t rdev)
    : InodeBase(ino, std::move(parentInode), name),
      state_(folly::in_place, this, mode, std::move(file), rdev) {}

folly::Future<fusell::Dispatcher::Attr> FileInode::getattr() {
  auto data = getOrLoadData();

  // Future optimization opportunity: right now, if we have not already
  // materialized the data from the entry, we have to materialize it
  // from the store.  If we augmented our metadata we could avoid this,
  // and this would speed up operations like `ls`.
  return data->ensureDataLoaded().then([ self = inodePtrFromThis(), data ]() {
    auto attr = fusell::Dispatcher::Attr{self->getMount()->getMountPoint()};
    attr.st = data->stat();
    attr.st.st_ino = self->getNodeId();
    return attr;
  });
}

folly::Future<fusell::Dispatcher::Attr> FileInode::setattr(
    const struct stat& attr,
    int to_set) {
  auto data = getOrLoadData();
  int openFlags = O_RDWR;

  // Minor optimization: if we know that the file is being completed truncated
  // as part of this operation, there's no need to fetch the underlying data,
  // so pass on the truncate flag our underlying open call
  if ((to_set & FUSE_SET_ATTR_SIZE) && attr.st_size == 0) {
    openFlags |= O_TRUNC;
  }

  return data->materializeForWrite(openFlags).then(
      [ self = inodePtrFromThis(), data, attr, to_set ]() {
        self->materializeInParent();

        auto result =
            fusell::Dispatcher::Attr{self->getMount()->getMountPoint()};
        result.st = data->setAttr(attr, to_set);
        result.st.st_ino = self->getNodeId();

        auto path = self->getPath();
        if (path.hasValue()) {
          self->getMount()->getJournal().wlock()->addDelta(
              std::make_unique<JournalDelta>(JournalDelta{path.value()}));
        }

        return result;
      });
}

folly::Future<std::string> FileInode::readlink() {
  shared_ptr<FileData> data;
  {
    auto state = state_.wlock();
    if (!S_ISLNK(state->mode)) {
      // man 2 readlink says:  EINVAL The named file is not a symbolic link.
      throw InodeError(EINVAL, inodePtrFromThis(), "not a symlink");
    }

    data = getOrLoadData(state);
  }

  // The symlink contents are simply the file contents!
  return data->ensureDataLoaded().then(
      [ self = inodePtrFromThis(), data ]() { return data->readAll(); });
}

std::shared_ptr<FileData> FileInode::getOrLoadData() {
  return getOrLoadData(state_.wlock());
}

std::shared_ptr<FileData> FileInode::getOrLoadData(
    const folly::Synchronized<State>::LockedPtr& state) {
  if (!state->data) {
    state->data = std::make_shared<FileData>(this, state->hash);
  }

  return state->data;
}

void FileInode::fileHandleDidClose() {
  {
    auto state = state_.wlock();
    if (state->data.unique()) {
      // We're the only remaining user, no need to keep it around
      state->data.reset();
    }
  }
}

AbsolutePath FileInode::getLocalPath() const {
  return getMount()->getOverlay()->getFilePath(getNodeId());
}

std::pair<bool, shared_ptr<FileData>> FileInode::isSameAsFast(
    const Hash& blobID,
    mode_t mode) {
  // When comparing mode bits, we only care about the
  // file type and owner permissions.
  auto relevantModeBits = [](mode_t m) { return (m & (S_IFMT | S_IRWXU)); };

  auto state = state_.wlock();
  if (relevantModeBits(state->mode) != relevantModeBits(mode)) {
    return std::make_pair(false, nullptr);
  }

  if (state->hash.hasValue()) {
    // This file is not materialized, so we can just compare hashes
    return std::make_pair(state->hash.value() == blobID, nullptr);
  }

  return std::make_pair(false, getOrLoadData(state));
}

bool FileInode::isSameAs(const Blob& blob, mode_t mode) {
  auto result = isSameAsFast(blob.getHash(), mode);
  if (!result.second) {
    return result.first;
  }

  return result.second->getSha1() == Hash::sha1(&blob.getContents());
}

folly::Future<bool> FileInode::isSameAs(const Hash& blobID, mode_t mode) {
  auto result = isSameAsFast(blobID, mode);
  if (!result.second) {
    return makeFuture(result.first);
  }

  return getMount()
      ->getObjectStore()
      ->getBlobMetadata(blobID)
      .then([self = inodePtrFromThis()](const BlobMetadata& metadata) {
        return self->getOrLoadData()->getSha1() == metadata.sha1;
      });
}

mode_t FileInode::getMode() const {
  return state_.rlock()->mode;
}

mode_t FileInode::getPermissions() const {
  return (getMode() & 07777);
}

folly::Optional<Hash> FileInode::getBlobHash() const {
  return state_.rlock()->hash;
}

folly::Future<std::shared_ptr<fusell::FileHandle>> FileInode::open(
    const struct fuse_file_info& fi) {
  shared_ptr<FileData> data;

// TODO: We currently should ideally call fileHandleDidClose() if we fail
// to create a FileHandle.  It's currently slightly tricky to do this right
// on all code paths.
//
// I think it will be better in the long run to just refactor how we do this.
// fileHandleDidClose() currently uses std::shared_ptr::unique(), which is
// deprecated in future versions of C++.
#if 0
  SCOPE_EXIT {
    data.reset();
    fileHandleDidClose();
  };
#endif

  {
    auto state = state_.wlock();

    if (S_ISLNK(state->mode)) {
      // Linux reports ELOOP if you try to open a symlink with O_NOFOLLOW set.
      // Since it isn't clear whether FUSE will allow this to happen, this
      // is a speculative defense against that happening; the O_PATH flag
      // does allow a file handle to be opened on a symlink on Linux,
      // but does not allow it to be used for real IO operations.  We're
      // punting on handling those situations here for now.
      throw InodeError(ELOOP, inodePtrFromThis(), "is a symlink");
    }

    data = getOrLoadData(state);
  }

  if (fi.flags & (O_RDWR | O_WRONLY | O_CREAT | O_TRUNC)) {
    return data->materializeForWrite(fi.flags).then(
        [ self = inodePtrFromThis(), data, flags = fi.flags ]() {
          self->materializeInParent();
          return shared_ptr<fusell::FileHandle>{
              std::make_shared<FileHandle>(self, data, flags)};
        });
  } else {
    return data->ensureDataLoaded().then(
        [ self = inodePtrFromThis(), data, flags = fi.flags ]() {
          return shared_ptr<fusell::FileHandle>{
              std::make_shared<FileHandle>(self, data, flags)};
        });
  }
}

void FileInode::materializeInParent() {
  auto renameLock = getMount()->acquireRenameLock();
  auto loc = getLocationInfo(renameLock);
  if (loc.parent && !loc.unlinked) {
    loc.parent->childMaterialized(renameLock, loc.name, getNodeId());
  }
}

std::shared_ptr<FileHandle> FileInode::finishCreate() {
  auto data = getOrLoadData();
  SCOPE_EXIT {
    data.reset();
    fileHandleDidClose();
  };
  return std::make_shared<FileHandle>(inodePtrFromThis(), data, 0);
}

Future<vector<string>> FileInode::listxattr() {
  // Currently, we only return a non-empty vector for regular files, and we
  // assume that the SHA-1 is present without checking the ObjectStore.
  vector<string> attributes;

  {
    auto state = state_.rlock();
    if (S_ISREG(state->mode)) {
      attributes.emplace_back(kXattrSha1.str());
    }
  }
  return attributes;
}

Future<string> FileInode::getxattr(StringPiece name) {
  // Currently, we only support the xattr for the SHA-1 of a regular file.
  if (name != kXattrSha1) {
    return makeFuture<string>(InodeError(kENOATTR, inodePtrFromThis()));
  }

  return getSHA1().then([](Hash hash) { return hash.toString(); });
}

Future<Hash> FileInode::getSHA1(bool failIfSymlink) {
  std::shared_ptr<FileData> data;
  folly::Optional<Hash> hash;
  {
    auto state = state_.wlock();
    if (failIfSymlink && !S_ISREG(state->mode)) {
      // We only define a SHA-1 value for regular files
      return makeFuture<Hash>(InodeError(kENOATTR, inodePtrFromThis()));
    }

    hash = state->hash;
    if (!hash.hasValue()) {
      data = getOrLoadData(state);
    }
  }

  if (hash.hasValue()) {
    return getMount()->getObjectStore()->getSha1ForBlob(hash.value());
  }

  return data->getSha1();
}
}
}
