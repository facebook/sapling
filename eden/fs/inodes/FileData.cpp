/*
 *  Copyright (c) 2016-present, Facebook, Inc.
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
#include <folly/Optional.h>
#include <folly/Range.h>
#include <folly/String.h>
#include <folly/experimental/logging/xlog.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <openssl/sha.h>

#include "eden/fs/fuse/BufVec.h"
#include "eden/fs/fuse/MountPoint.h"
#include "eden/fs/fuse/fuse_headers.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/utils/Bug.h"
#include "eden/fs/utils/XAttr.h"

using folly::ByteRange;
using folly::checkUnixError;
using folly::Future;
using folly::makeFuture;
using folly::Unit;
using folly::StringPiece;

namespace facebook {
namespace eden {

FileData::FileData(FileInode* inode) : inode_(inode) {}

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
  auto state = inode_->state_.wlock();

  CHECK(state->file) << "MUST have a materialized file at this point";

  // We most likely need the current information to apply the requested
  // changes below, so just fetch it here first.
  struct stat currentStat;
  checkUnixError(fstat(state->file.fd(), &currentStat));

  if (to_set & FUSE_SET_ATTR_SIZE) {
    checkUnixError(
        ftruncate(state->file.fd(), attr.st_size + Overlay::kHeaderLength));
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
    // The mode data is stored only in inode_->state_.
    // (We don't set mode bits on the overlay file as that may incorrectly
    // prevent us from reading or writing the overlay data).
    // Make sure we preserve the file type bits, and only update permissions.
    state->mode = (state->mode & S_IFMT) | (07777 & attr.st_mode);
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

    checkUnixError(futimens(state->file.fd(), times));
  }

  // We need to return the now-current stat information for this file.
  struct stat returnedStat;
  checkUnixError(fstat(state->file.fd(), &returnedStat));
  returnedStat.st_mode = state->mode;
  returnedStat.st_size -= Overlay::kHeaderLength;

  return returnedStat;
}

struct stat FileData::stat() {
  auto st = inode_->getMount()->getMountPoint()->initStatData();
  st.st_nlink = 1;

  auto state = inode_->state_.rlock();

  if (state->file) {
    // stat() the overlay file.
    //
    // TODO: We need to get timestamps accurately here.
    // The timestamps on the underlying file are not correct, because we keep
    // file_ open for a long time, and do not close it when FUSE file handles
    // close.  (Timestamps are typically only updated on close operations.)
    // This results our reported timestamps not changing correctly after the
    // file is changed through FUSE APIs.
    //
    // We probably should update the overlay file to include a header,
    // so we can store the atime, mtime, and ctime in the header data.
    // Otherwise we won't be able to report the ctime accurately if we just
    // keep using the overlay file timestamps.
    checkUnixError(fstat(state->file.fd(), &st));

    if (st.st_size < Overlay::kHeaderLength) {
      auto filePath = inode_->getLocalPath();
      EDEN_BUG() << "Overlay file " << inode_->getLocalPath()
                 << " is too short for header: size=" << st.st_size;
    }

    st.st_size -= Overlay::kHeaderLength;
    st.st_mode = state->mode;
    st.st_rdev = state->rdev;

    return st;
  }

  CHECK(state->blob);
  st.st_mode = state->mode;

  auto buf = state->blob->getContents();
  st.st_size = buf.computeChainDataLength();

  // Report atime, mtime, and ctime as the time when we first loaded this
  // FileInode.  It hasn't been materialized yet, so this is a reasonble time
  // to use.  Once it is materialized we use the timestamps on the underlying
  // overlay file, which the kernel keeps up-to-date.
  auto epochTime = state->creationTime.time_since_epoch();
  auto epochSeconds =
      std::chrono::duration_cast<std::chrono::seconds>(epochTime);
  st.st_atime = epochSeconds.count();
  st.st_mtime = epochSeconds.count();
  st.st_ctime = epochSeconds.count();
#if defined(_BSD_SOURCE) || defined(_SVID_SOURCE) || \
    _POSIX_C_SOURCE >= 200809L || _XOPEN_SOURCE >= 700
  auto nsec = std::chrono::duration_cast<std::chrono::nanoseconds>(
      epochTime - epochSeconds);
  st.st_atim.tv_nsec = nsec.count();
  st.st_mtim.tv_nsec = nsec.count();
  st.st_ctim.tv_nsec = nsec.count();
#endif

  // NOTE: we don't set rdev to anything special here because we
  // don't support committing special device nodes.

  return st;
}

void FileData::flush(uint64_t /* lock_owner */) {
  // We have no write buffers, so there is nothing for us to flush,
  // but let's take this opportunity to update the sha1 attribute.
  auto state = inode_->state_.wlock();
  if (state->file && !state->sha1Valid) {
    recomputeAndStoreSha1(state);
  }
}

void FileData::fsync(bool datasync) {
  auto state = inode_->state_.wlock();
  if (!state->file) {
    // If we don't have an overlay file then we have nothing to sync.
    return;
  }

  auto res =
#ifndef __APPLE__
      datasync ? ::fdatasync(state->file.fd()) :
#endif
               ::fsync(state->file.fd());
  checkUnixError(res);

  // let's take this opportunity to update the sha1 attribute.
  if (!state->sha1Valid) {
    recomputeAndStoreSha1(state);
  }
}

std::unique_ptr<folly::IOBuf> FileData::readIntoBuffer(size_t size, off_t off) {
  auto state = inode_->state_.rlock();

  if (state->file) {
    auto buf = folly::IOBuf::createCombined(size);
    auto res = ::pread(
        state->file.fd(),
        buf->writableBuffer(),
        size,
        off + Overlay::kHeaderLength);

    checkUnixError(res);
    buf->append(res);
    return buf;
  }

  auto buf = state->blob->getContents();
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
  auto state = inode_->state_.rlock();
  if (state->file) {
    std::string result;
    auto rc = lseek(state->file.fd(), Overlay::kHeaderLength, SEEK_SET);
    folly::checkUnixError(rc, "unable to seek in materialized FileData");
    folly::readFile(state->file.fd(), result);
    return result;
  }

  const auto& contentsBuf = state->blob->getContents();
  folly::io::Cursor cursor(&contentsBuf);
  return cursor.readFixedString(contentsBuf.computeChainDataLength());
}

fusell::BufVec FileData::read(size_t size, off_t off) {
  auto buf = readIntoBuffer(size, off);
  return fusell::BufVec(std::move(buf));
}

size_t FileData::write(fusell::BufVec&& buf, off_t off) {
  auto state = inode_->state_.wlock();
  if (!state->file) {
    // Not open for write
    folly::throwSystemErrorExplicit(EINVAL);
  }

  state->sha1Valid = false;
  auto vec = buf.getIov();
  auto xfer = ::pwritev(
      state->file.fd(), vec.data(), vec.size(), off + Overlay::kHeaderLength);
  checkUnixError(xfer);
  return xfer;
}

size_t FileData::write(folly::StringPiece data, off_t off) {
  auto state = inode_->state_.wlock();
  if (!state->file) {
    // Not open for write
    folly::throwSystemErrorExplicit(EINVAL);
  }

  state->sha1Valid = false;
  auto xfer = ::pwrite(
      state->file.fd(), data.data(), data.size(), off + Overlay::kHeaderLength);
  checkUnixError(xfer);
  return xfer;
}

Future<Unit> FileData::ensureDataLoaded() {
  auto state = inode_->state_.wlock();

  if (!state->hash.hasValue()) {
    // We should always have the file open if we are materialized.
    CHECK(state->file);
    return makeFuture();
  }

  if (state->blob) {
    DCHECK_EQ(state->blob->getHash(), state->hash.value());
    return makeFuture();
  }

  // Load the blob data.
  auto blobFuture = getObjectStore()->getBlob(state->hash.value());

  // TODO: We really should defer this using a Future rather than calling get()
  // here and blocking until the load completes.  However, for that to work we
  // will need to add some extra data tracking whether or not we are already in
  // the process of loading the data.  We need to avoid multiple threads all
  // trying to load the data at the same time.
  //
  // For now doing a blocking load with the inode_->state_ lock held ensures
  // that only one thread can load the data at a time.  It's pretty unfortunate
  // to block with the lock held, though :-(
  state->blob = blobFuture.get();
  return makeFuture();
}

Future<Unit> FileData::materializeForWrite(int openFlags) {
  auto state = inode_->state_.wlock();

  // If we already have a materialized overlay file then we don't
  // need to do much
  if (state->file) {
    CHECK(!state->hash.hasValue());
    if ((openFlags & O_TRUNC) != 0) {
      // truncating a file that we already have open
      state->sha1Valid = false;
      checkUnixError(ftruncate(state->file.fd(), Overlay::kHeaderLength));
      auto emptySha1 = Hash::sha1(ByteRange{});
      storeSha1(state, emptySha1);
    } else {
      // no truncate option,overlay file contain old header
      // we have to update only header but not contents
    }
    return makeFuture();
  }

  // Add header to the overlay File.
  struct timespec zeroTime = {0, 0};
  auto header = Overlay::createHeader(
      Overlay::kHeaderIdentifierFile,
      Overlay::kHeaderVersion,
      zeroTime,
      zeroTime,
      zeroTime);
  auto iov = header.getIov();

  // We must not be materialized yet
  CHECK(state->hash.hasValue());

  Hash sha1;
  auto filePath = inode_->getLocalPath();

  if ((openFlags & O_TRUNC) != 0) {
    folly::writeFileAtomic(filePath.stringPiece(), iov.data(), iov.size());
    state->file = Overlay::openFile(filePath.stringPiece());
    sha1 = Hash::sha1(ByteRange{});
  } else {
    if (!state->blob) {
      // TODO: Load the blob using the non-blocking Future APIs.
      // However, just as in ensureDataLoaded() above we will also need
      // to add a mechanism to wait for already in-progress loads.
      auto blobFuture = getObjectStore()->getBlob(state->hash.value());
      state->blob = blobFuture.get();
    }

    // Write the blob contents out to the overlay
    auto contents = state->blob->getContents().getIov();
    iov.insert(iov.end(), contents.begin(), contents.end());

    folly::writeFileAtomic(
        filePath.stringPiece(), iov.data(), iov.size(), 0600);
    state->file = Overlay::openFile(filePath.stringPiece());

    sha1 = getObjectStore()->getSha1ForBlob(state->hash.value());
  }

  // Copy and apply the sha1 to the new file.  This saves us from
  // recomputing it again in the case that something opens the file
  // read/write and closes it without changing it.
  storeSha1(state, sha1);

  // Update the FileInode to indicate that we are materialized now
  state->blob.reset();
  state->hash = folly::none;

  return makeFuture();
}

Hash FileData::getSha1() {
  auto state = inode_->state_.wlock();
  if (state->file) {
    std::string shastr;
    if (state->sha1Valid) {
      shastr = fgetxattr(state->file.fd(), kXattrSha1);
    }
    if (shastr.empty()) {
      return recomputeAndStoreSha1(state);
    } else {
      return Hash(shastr);
    }
  }

  CHECK(state->hash.hasValue());
  return getObjectStore()->getSha1ForBlob(state->hash.value());
}

ObjectStore* FileData::getObjectStore() const {
  return inode_->getMount()->getObjectStore();
}

Hash FileData::recomputeAndStoreSha1(
    const folly::Synchronized<FileInode::State>::LockedPtr& state) {
  uint8_t buf[8192];
  off_t off = Overlay::kHeaderLength;
  SHA_CTX ctx;
  SHA1_Init(&ctx);

  while (true) {
    // Using pread here so that we don't move the file position;
    // the file descriptor is shared between multiple file handles
    // and while we serialize the requests to FileData, it seems
    // like a good property of this function to avoid changing that
    // state.
    auto len = folly::preadNoInt(state->file.fd(), buf, sizeof(buf), off);
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
  auto sha1 = Hash(folly::ByteRange(digest, sizeof(digest)));
  storeSha1(state, sha1);
  return sha1;
}

void FileData::storeSha1(
    const folly::Synchronized<FileInode::State>::LockedPtr& state,
    Hash sha1) {
  try {
    fsetxattr(state->file.fd(), kXattrSha1, sha1.toString());
    state->sha1Valid = true;
  } catch (const std::exception& ex) {
    // If something goes wrong storing the attribute just log a warning
    // and leave sha1Valid as false.  We'll have to recompute the value
    // next time we need it.
    XLOG(WARNING) << "error setting SHA1 attribute in the overlay: "
                  << folly::exceptionStr(ex);
  }
}
}
}
