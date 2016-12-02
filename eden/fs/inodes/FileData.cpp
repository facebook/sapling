/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "FileData.h"

#include <folly/Exception.h>
#include <folly/FileUtil.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <openssl/sha.h>
#include "Overlay.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fuse/MountPoint.h"
#include "eden/fuse/fuse_headers.h"
#include "eden/utils/XAttr.h"

namespace facebook {
namespace eden {

using folly::checkUnixError;

FileData::FileData(std::mutex& mutex, EdenMount* mount, TreeInode::Entry* entry)
    : mutex_(mutex), mount_(mount), entry_(entry) {}

FileData::FileData(
    std::mutex& mutex,
    EdenMount* mount,
    TreeInode::Entry* entry,
    folly::File&& file)
    : mutex_(mutex), mount_(mount), entry_(entry), file_(std::move(file)) {}

// Conditionally updates target with either the value provided by
// the caller, or with the current time value, depending on the value
// of the flags in to_set.  Valid flag values are defined in fuse_lowlevel.h
// and have symbolic names matching FUSE_SET_*.
// useAttrFlag is the bitmask that indicates whether we should use the value
// from wantedTimeSpec.  useNowFlag is the bitmask that indicates whether we
// should use the current time instead.
// If neither flag is present, we will preserve the current value in target.
static void resolveTimeForSetAttr(
    struct timespec& target,
    int to_set,
    int useAttrFlag,
    int useNowFlag,
    const struct timespec& wantedTimeSpec) {
  if (to_set & useAttrFlag) {
    target = wantedTimeSpec;
  } else if (to_set & useNowFlag) {
    clock_gettime(CLOCK_REALTIME, &target);
  }
}

// Valid values for to_set are found in fuse_lowlevel.h and have symbolic
// names matching FUSE_SET_*.
struct stat FileData::setAttr(const struct stat& attr, int to_set) {
  std::unique_lock<std::mutex> lock(mutex_);

  CHECK(file_) << "MUST have a materialized file at this point";

  // We most likely need the current information to apply the requested
  // changes below, so just fetch it here first.
  struct stat currentStat;
  checkUnixError(fstat(file_.fd(), &currentStat));

  if (to_set & FUSE_SET_ATTR_SIZE) {
    checkUnixError(ftruncate(file_.fd(), attr.st_size));
  }

  if (to_set & (FUSE_SET_ATTR_UID | FUSE_SET_ATTR_GID)) {
    if ((to_set & FUSE_SET_ATTR_UID && attr.st_uid != currentStat.st_uid) ||
        (to_set & FUSE_SET_ATTR_GID && attr.st_gid != currentStat.st_gid)) {
      folly::throwSystemErrorExplicit(
          EACCES, "changing the owner/group is not supported");
    }

    // Otherwise: there is no change
  }

  if (to_set & FUSE_SET_ATTR_MODE) {
    checkUnixError(fchmod(file_.fd(), attr.st_mode));
  }

  if (to_set & (FUSE_SET_ATTR_ATIME | FUSE_SET_ATTR_MTIME |
                FUSE_SET_ATTR_ATIME_NOW | FUSE_SET_ATTR_MTIME_NOW)) {
    // Changing various time components.
    // Element 0 is the atime, element 1 is the mtime.
    struct timespec times[2] = {currentStat.st_atim, currentStat.st_mtim};

    resolveTimeForSetAttr(
        times[0],
        to_set,
        FUSE_SET_ATTR_ATIME,
        FUSE_SET_ATTR_ATIME_NOW,
        attr.st_atim);

    resolveTimeForSetAttr(
        times[1],
        to_set,
        FUSE_SET_ATTR_MTIME,
        FUSE_SET_ATTR_MTIME_NOW,
        attr.st_mtim);

    checkUnixError(futimens(file_.fd(), times));
  }

  // We need to return the now-current stat information for this file.
  struct stat returnedStat;
  checkUnixError(fstat(file_.fd(), &returnedStat));

  return returnedStat;
}

struct stat FileData::stat() {
  auto st = mount_->getMountPoint()->initStatData();
  st.st_nlink = 1;

  std::unique_lock<std::mutex> lock(mutex_);

  if (file_) {
    // stat() the overlay file.
    checkUnixError(fstat(file_.fd(), &st));
    return st;
  }

  CHECK(blob_);
  CHECK(entry_);

  st.st_mode = entry_->mode;

  auto buf = blob_->getContents();
  st.st_size = buf.computeChainDataLength();
  // TODO: set atime, mtime, and ctime

  return st;
}

void FileData::flush(uint64_t /* lock_owner */) {
  // We have no write buffers, so there is nothing for us to flush,
  // but let's take this opportunity to update the sha1 attribute.
  std::unique_lock<std::mutex> lock(mutex_);
  if (file_ && !sha1Valid_) {
    recomputeAndStoreSha1();
  }
}

void FileData::fsync(bool datasync) {
  std::unique_lock<std::mutex> lock(mutex_);
  if (!file_) {
    // If we don't have an overlay file then we have nothing to sync.
    return;
  }

  auto res =
#ifndef __APPLE__
      datasync ? ::fdatasync(file_.fd()) :
#endif
               ::fsync(file_.fd());
  checkUnixError(res);

  // let's take this opportunity to update the sha1 attribute.
  if (!sha1Valid_) {
    recomputeAndStoreSha1();
  }
}

std::unique_ptr<folly::IOBuf> FileData::readIntoBuffer(size_t size, off_t off) {
  std::unique_lock<std::mutex> lock(mutex_);

  if (file_) {
    auto buf = folly::IOBuf::createCombined(size);
    auto res = ::pread(file_.fd(), buf->writableBuffer(), size, off);
    checkUnixError(res);
    buf->append(res);
    return buf;
  }

  auto buf = blob_->getContents();
  folly::io::Cursor cursor(&buf);

  if (!cursor.canAdvance(off)) {
    // Seek beyond EOF.  Return an empty result.
    return folly::IOBuf::wrapBuffer("", 0);
  }

  cursor.skip(off);

  std::unique_ptr<folly::IOBuf> result;
  cursor.cloneAtMost(result, size);
  return result;
}

std::string FileData::readAll() {
  std::unique_lock<std::mutex> lock(mutex_);
  if (file_) {
    std::string result;
    auto rc = lseek(file_.fd(), 0, SEEK_SET);
    folly::checkUnixError(rc, "unable to seek in materialized FileData");
    folly::readFile(file_.fd(), result);
    return result;
  }

  const auto& contentsBuf = blob_->getContents();
  folly::io::Cursor cursor(&contentsBuf);
  return cursor.readFixedString(contentsBuf.computeChainDataLength());
}

fusell::BufVec FileData::read(size_t size, off_t off) {
  auto buf = readIntoBuffer(size, off);
  return fusell::BufVec(std::move(buf));
}

size_t FileData::write(fusell::BufVec&& buf, off_t off) {
  std::unique_lock<std::mutex> lock(mutex_);
  if (!file_) {
    // Not open for write
    folly::throwSystemErrorExplicit(EINVAL);
  }

  sha1Valid_ = false;
  auto vec = buf.getIov();
  auto xfer = ::pwritev(file_.fd(), vec.data(), vec.size(), off);
  checkUnixError(xfer);
  return xfer;
}

size_t FileData::write(folly::StringPiece data, off_t off) {
  std::unique_lock<std::mutex> lock(mutex_);
  if (!file_) {
    // Not open for write
    folly::throwSystemErrorExplicit(EINVAL);
  }

  sha1Valid_ = false;
  auto xfer = ::pwrite(file_.fd(), data.data(), data.size(), off);
  checkUnixError(xfer);
  return xfer;
}

void FileData::materializeForRead(
    int open_flags,
    RelativePathPiece path,
    std::shared_ptr<Overlay> overlay) {
  DCHECK((open_flags & (O_WRONLY | O_RDWR | O_CREAT | O_TRUNC)) == 0)
      << "this is the read only materialization method";
  std::unique_lock<std::mutex> lock(mutex_);

  // If we have a tree entry, assume that we will need the blob data...
  bool needBlob = entry_->hash.hasValue();

  if (entry_->materialized) {
    // Canonical data for this entry lives in the file in the overlay, so there
    // is not need to go to the store to satisfy it.
    needBlob = false;

    // If we have a pre-existing overlay file, we do not need to go to
    // the store at all.
    if (!file_) {
      auto filePath = overlay->getContentDir() + path;
      // O_NOFOLLOW because it never makes sense for the kernel to ask
      // a fuse server to open a file that is a symlink to something else.
      file_ = folly::File(filePath.c_str(), O_RDWR | O_NOFOLLOW);
      // A freshly opened file has a valid sha1 attribute (assuming no
      // outside interference)
      sha1Valid_ = true;
    }
  }

  if (needBlob && !blob_) {
    // Load the blob data.
    blob_ = mount_->getObjectStore()->getBlob(entry_->hash.value());
  }
}

void FileData::materializeForWrite(
    int open_flags,
    RelativePathPiece path,
    std::shared_ptr<Overlay> overlay) {
  std::unique_lock<std::mutex> lock(mutex_);

  // If we have a tree entry, assume that we will need the blob data...
  bool needBlob = entry_->hash.hasValue();
  // ... and that we don't need an overlay file handle.
  bool needFile = false;

  if ((open_flags & O_TRUNC) != 0) {
    // Truncation is a write operation, so we will need an overlay file.
    needFile = true;
    // No need to materialize the blob from the store if we're just
    // going to truncate it anyway.
    needBlob = false;
  }
  if ((open_flags & (O_RDWR | O_WRONLY)) != 0) {
    // Write operations require an overlay file.
    needFile = true;
  }

  if (needBlob && entry_->materialized) {
    // Data was deleted, no need to go to the store to satisfy it.
    needBlob = false;
  }

  auto filePath = overlay->getContentDir() + path;

  // If we have a pre-existing overlay file, we do not need to go to
  // the store at all.
  if (!file_ && entry_->materialized) {
    // O_NOFOLLOW because it never makes sense for the kernel to ask
    // a fuse server to open a file that is a symlink to something else.
    file_ = folly::File(filePath.c_str(), O_RDWR | O_NOFOLLOW, 0600);
    // since we have a pre-existing overlay file, we don't need the blob.
    needBlob = false;
    // A freshly opened file has a valid sha1 attribute (assuming no
    // outside interference)
    sha1Valid_ = true;
  }

  if (needBlob && !blob_) {
    // Load the blob data.
    blob_ = mount_->getObjectStore()->getBlob(entry_->hash.value());
  }

  if (needFile && !file_) {
    if (!entry_ && (open_flags & O_CREAT) == 0) {
      // If we get here, we do not have any usable backing from the store
      // and do not have a pre-existing overlay file.
      // The current file open request isn't asking us to create a file,
      // therefore we should not create one as we are about to do below.
      // I don't know if the kernel is smart enough to detect and prevent
      // this at a higher level or not, but it feels safer to be sure here.
      folly::throwSystemErrorExplicit(ENOENT);
    }

    // We need an overlay file and don't yet have one.
    // We always create our internal file handle read/write regardless of
    // the mode that the client is requesting.
    auto file = folly::File(filePath.c_str(), O_RDWR | O_CREAT, 0600);
    entry_->materialized = true;

    // We typically need to populate our newly opened file with the data
    // from the overlay.  The O_TRUNC check above may have set needBlob
    // to false, so we'll end up just creating an empty file and skipping
    // the blob copy.
    if (needBlob) {
      auto buf = blob_->getContents();
      auto iov = buf.getIov();
      auto wrote = folly::writevNoInt(file.fd(), iov.data(), iov.size());
      auto err = errno;
      if (wrote != buf.computeChainDataLength()) {
        folly::throwSystemErrorExplicit(
            wrote == -1 ? err : EIO, "failed to materialize ", path);
      }

      // Copy and apply the sha1 to the new file.  This saves us from
      // recomputing it again in the case that something opens the file
      // read/write and closes it without changing it.
      auto sha1 =
          mount_->getObjectStore()->getSha1ForBlob(entry_->hash.value());
      fsetxattr(file.fd(), kXattrSha1, sha1->toString());
      sha1Valid_ = true;
    }

    // transfer ownership of the fd to us.  Do this after dealing with any
    // errors during materialization so that our internal state is easier
    // to reason about.
    file_ = std::move(file);
    sha1Valid_ = false;
  } else if (file_ && (open_flags & O_TRUNC) != 0) {
    // truncating a file that we already have open
    sha1Valid_ = false;
    checkUnixError(ftruncate(file_.fd(), 0));
  }
}

Hash FileData::getSha1() {
  std::unique_lock<std::mutex> lock(mutex_);
  return getSha1Locked(lock);
}

Hash FileData::getSha1Locked(const std::unique_lock<std::mutex>&) {
  if (file_) {
    std::string shastr;
    if (sha1Valid_) {
      shastr = fgetxattr(file_.fd(), kXattrSha1);
    }
    if (shastr.empty()) {
      return recomputeAndStoreSha1();
    } else {
      return Hash(shastr);
    }
  }

  CHECK(entry_->hash.hasValue());
  auto sha1 = mount_->getObjectStore()->getSha1ForBlob(entry_->hash.value());
  return *sha1.get();
}

Hash FileData::recomputeAndStoreSha1() {
  uint8_t buf[8192];
  off_t off = 0;
  SHA_CTX ctx;
  SHA1_Init(&ctx);

  while (true) {
    // Using pread here so that we don't move the file position;
    // the file descriptor is shared between multiple file handles
    // and while we serialize the requests to FileData, it seems
    // like a good property of this function to avoid changing that
    // state.
    auto len = folly::preadNoInt(file_.fd(), buf, sizeof(buf), off);
    if (len == 0) {
      break;
    }
    if (len == -1) {
      folly::throwSystemError();
    }
    SHA1_Update(&ctx, buf, len);
    off += len;
  }

  uint8_t digest[SHA_DIGEST_LENGTH];
  SHA1_Final(digest, &ctx);
  auto hash = Hash(folly::ByteRange(digest, sizeof(digest)));
  auto sha1 = hash.toString();

  fsetxattr(file_.fd(), kXattrSha1, sha1);
  sha1Valid_ = true;

  return hash;
}
}
}
