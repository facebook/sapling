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
#include "eden/fs/overlay/Overlay.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/utils/XAttr.h"

namespace facebook {
namespace eden {

using folly::checkUnixError;

FileData::FileData(
    std::mutex& mutex,
    std::shared_ptr<LocalStore> store,
    std::shared_ptr<Overlay> overlay,
    const TreeEntry* entry)
    : mutex_(mutex), store_(store), overlay_(overlay), entry_(entry) {}

struct stat FileData::stat() {
  struct stat st;

  std::unique_lock<std::mutex> lock(mutex_);

  if (file_) {
    // stat() the overlay file.
    checkUnixError(fstat(file_.fd(), &st));
    return st;
  }

  CHECK(blob_);
  CHECK_NOTNULL(entry_);

  switch (entry_->getFileType()) {
    case FileType::SYMLINK:
      st.st_mode = S_IFLNK;
      break;
    case FileType::REGULAR_FILE:
      st.st_mode = S_IFREG;
      break;
    default:
      folly::throwSystemErrorExplicit(
          EINVAL,
          "TreeEntry has an invalid file type: ",
          entry_->getFileType());
  }

  // Bit 1 is the executable flag.  Flesh out all the permission bits based on
  // the executable bit being set or not.
  if (entry_->getOwnerPermissions() & 1) {
    st.st_mode |= 0755;
  } else {
    st.st_mode |= 0644;
  }

  auto buf = blob_->getContents();
  st.st_size = buf.computeChainDataLength();

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

fusell::BufVec FileData::read(size_t size, off_t off) {
  std::unique_lock<std::mutex> lock(mutex_);

  if (file_) {
    auto buf = folly::IOBuf::createCombined(size);
    auto res = ::pread(file_.fd(), buf->writableBuffer(), size, off);
    checkUnixError(res);
    buf->append(res);
    return fusell::BufVec(std::move(buf));
  }

  auto buf = blob_->getContents();
  folly::io::Cursor cursor(&buf);

  if (!cursor.canAdvance(off)) {
    // Seek beyond EOF.  Return an empty result.
    return fusell::BufVec(folly::IOBuf::wrapBuffer("", 0));
  }

  cursor.skip(off);

  std::unique_ptr<folly::IOBuf> result;
  cursor.cloneAtMost(result, size);
  return fusell::BufVec(std::move(result));
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

void FileData::materialize(int open_flags, RelativePathPiece path) {
  std::unique_lock<std::mutex> lock(mutex_);

  // If we have a tree entry, assume that we will need the blob data...
  bool need_blob = entry_ != nullptr;
  // ... and that we don't need an overlay file handle.
  bool need_file = false;

  if ((open_flags & O_TRUNC) != 0) {
    // Truncation is a write operation, so we will need an overlay file.
    need_file = true;
    // No need to materialize the blob from the store if we're just
    // going to truncate it anyway.
    need_blob = false;
  }
  if ((open_flags & (O_RDWR | O_WRONLY)) != 0) {
    // Write operations require an overlay file.
    need_file = true;
  }

  if (need_blob && overlay_->isWhiteout(path)) {
    // Data was deleted, no need to go to the store to satisfy it.
    need_blob = false;
  }

  // If we have a pre-existing overlay file, we do not need to go to
  // the store at all.
  if (!file_) {
    try {
      // Test whether an overlay file exists by trying to open it.
      // O_NOFOLLOW because it never makes sense for the kernel to ask
      // a fuse server to open a file that is a symlink to something else.
      file_ = overlay_->openFile(path, O_RDWR | O_NOFOLLOW, 0600);
      // since we have a pre-existing overlay file, we don't need the blob.
      need_blob = false;
      // A freshly opened file has a valid sha1 attribute (assuming no
      // outside interference)
      sha1Valid_ = true;
    } catch (const std::system_error& err) {
      if (err.code().value() != ENOENT) {
        throw;
      }
      // Else: doesn't exist in the overlay right now
    }
  }

  if (need_blob && !blob_) {
    // Load the blob data.
    blob_ = store_->getBlob(entry_->getHash());
  }

  if (need_file && !file_) {
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
    auto file = overlay_->openFile(path, O_RDWR | O_CREAT, 0600);

    // We typically need to populate our newly opened file with the data
    // from the overlay.  The O_TRUNC check above may have set need_blob
    // to false, so we'll end up just creating an empty file and skipping
    // the blob copy.
    if (need_blob) {
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
      auto sha1 = store_->getSha1ForBlob(entry_->getHash());
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

std::string FileData::getSha1() {
  std::unique_lock<std::mutex> lock(mutex_);
  return getSha1Locked(lock);
}

std::string FileData::getSha1Locked(const std::unique_lock<std::mutex>&) {
  if (file_) {
    std::string shastr;
    if (sha1Valid_) {
      shastr = fgetxattr(file_.fd(), kXattrSha1);
    }
    if (shastr.empty()) {
      shastr = recomputeAndStoreSha1();
    }
    return shastr;
  }

  CHECK_NOTNULL(entry_);
  auto sha1 = store_->getSha1ForBlob(entry_->getHash());
  return sha1->toString();
}

std::string FileData::recomputeAndStoreSha1() {
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
  auto sha1 = Hash(folly::ByteRange(digest, sizeof(digest))).toString();

  fsetxattr(file_.fd(), kXattrSha1, sha1);
  sha1Valid_ = true;

  return sha1;
}
}
}
