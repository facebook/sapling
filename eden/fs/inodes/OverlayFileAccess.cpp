/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/OverlayFileAccess.h"

#include <folly/Expected.h>
#include <folly/Range.h>
#include <folly/logging/xlog.h>
#include <gflags/gflags.h>
#include <openssl/sha.h>

#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/InodeError.h"
#include "eden/fs/inodes/InodePtr.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/inodes/OverlayFile.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/utils/Bug.h"
#include "folly/FileUtil.h"

namespace facebook {
namespace eden {

/*
 * OverlayFileAccess should be careful not to perform overlay IO operations
 * while its own state lock is held. Doing so serializes IO operations to the
 * overlay which impacts throughput under concurrent operations.
 */

DEFINE_uint64(overlayFileCacheSize, 100, "");

void OverlayFileAccess::Entry::Info::invalidateMetadata() {
  ++version;
  size = std::nullopt;
  sha1 = std::nullopt;
}

OverlayFileAccess::State::State(size_t cacheSize) : entries{cacheSize} {
  if (cacheSize == 0) {
    throw std::range_error{"overlayFileCacheSize must be at least 1"};
  }
}

OverlayFileAccess::OverlayFileAccess(Overlay* overlay)
    : overlay_{overlay}, state_{folly::in_place, FLAGS_overlayFileCacheSize} {}

OverlayFileAccess::~OverlayFileAccess() = default;

void OverlayFileAccess::createEmptyFile(InodeNumber ino) {
  auto file = overlay_->createOverlayFile(ino, folly::ByteRange{});
  auto state = state_.wlock();
  CHECK(!state->entries.exists(ino))
      << "Cannot create overlay file " << ino << " when it's already open!";
  state->entries.set(
      ino, std::make_shared<Entry>(std::move(file), size_t{0}, kEmptySha1));
}

void OverlayFileAccess::createFile(
    InodeNumber ino,
    const Blob& blob,
    const std::optional<Hash>& sha1) {
  auto file = overlay_->createOverlayFile(ino, blob.getContents());
  auto state = state_.wlock();
  CHECK(!state->entries.exists(ino))
      << "Cannot create overlay file " << ino << " when it's already open!";
  state->entries.set(
      ino, std::make_shared<Entry>(std::move(file), blob.getSize(), sha1));
}

off_t OverlayFileAccess::getFileSize(FileInode& inode) {
  auto entry = getEntryForInode(inode.getNodeId());
  uint64_t version;
  {
    auto info = entry->info.rlock();
    if (info->size.has_value()) {
      return *info->size;
    }
    version = info->version;
  }

  // Size is not known, so fstat the file. Do so while the lock is not held to
  // improve concurrency.
  auto ret = entry->file.fstat();
  if (ret.hasError()) {
    throw InodeError(
        ret.error(), inode.inodePtrFromThis(), "unable to fstat overlay file");
  }
  auto st = ret.value();
  if (st.st_size < static_cast<off_t>(FsOverlay::kHeaderLength)) {
    // Truncated overlay files can sometimes occur after a hard reboot
    // where the overlay file data was not flushed to disk before the
    // system powered off.
    XLOG(ERR) << "overlay file for " << inode.getNodeId()
              << " is too short for header: size=" << st.st_size;
    throw InodeError(EIO, inode.inodePtrFromThis(), "corrupt overlay file");
  }

  auto size = st.st_size - static_cast<off_t>(FsOverlay::kHeaderLength);

  // Update the cache if the version still matches.
  auto info = entry->info.wlock();
  if (version == info->version) {
    info->size = size;
  }
  return size;
}

Hash OverlayFileAccess::getSha1(FileInode& inode) {
  auto entry = getEntryForInode(inode.getNodeId());
  uint64_t version;
  {
    auto info = entry->info.rlock();
    if (info->sha1.has_value()) {
      return *info->sha1;
    }
    version = info->version;
  }

  // SHA-1 is not known, so recompute it. Do so while the lock is not held to
  // improve concurrency.

  SHA_CTX ctx;
  SHA1_Init(&ctx);

  off_t off = FsOverlay::kHeaderLength;
  while (true) {
    // Using pread here so that we don't move the file position;
    // the file descriptor is shared between multiple file handles
    // and while we serialize the requests to FileData, it seems
    // like a good property of this function to avoid changing that
    // state.
    uint8_t buf[8192];
    auto ret = entry->file.preadNoInt(&buf, sizeof(buf), off);
    if (ret.hasError()) {
      throw InodeError(
          ret.error(),
          inode.inodePtrFromThis(),
          "pread failed during SHA-1 calculation");
    }
    auto len = ret.value();
    if (len == 0) {
      break;
    }
    SHA1_Update(&ctx, buf, len);
    off += len;
  }

  static_assert(Hash::RAW_SIZE == SHA_DIGEST_LENGTH);
  Hash sha1;
  SHA1_Final(sha1.mutableBytes().begin(), &ctx);

  // Update the cache if the version still matches.
  auto info = entry->info.wlock();
  if (version == info->version) {
    info->sha1 = sha1;
  }
  return sha1;
}

std::string OverlayFileAccess::readAllContents(FileInode& inode) {
  auto entry = getEntryForInode(inode.getNodeId());

  // Note that this code requires a write lock on the entry because the lseek()
  // call modifies the file offset of the file descriptor. Otherwise, concurrent
  // readAllContents() calls would step on each other.
  //
  // This violates our rule of not doing IO while locks are held, but
  // readAllContents() is rare, primarily for files like .gitignore that Eden
  // must read.
  //
  // TODO: implement readFile with pread instead of lseek.
  auto info = entry->info.wlock();

  auto rc = entry->file.lseek(FsOverlay::kHeaderLength, SEEK_SET);
  if (rc.hasError()) {
    throw InodeError(
        rc.error(),
        inode.inodePtrFromThis(),
        "unable to seek in materialized FileInode");
  }
  auto result = entry->file.readFile();

  if (result.hasError()) {
    throw InodeError(
        result.error(),
        inode.inodePtrFromThis(),
        "unable to read overlay file");
  }
  return result.value();
}

BufVec OverlayFileAccess::read(FileInode& inode, size_t size, off_t off) {
  auto entry = getEntryForInode(inode.getNodeId());

  auto buf = folly::IOBuf::createCombined(size);
  auto res = entry->file.preadNoInt(
      buf->writableBuffer(), size, off + FsOverlay::kHeaderLength);

  if (res.hasError()) {
    throw InodeError(
        res.error(),
        inode.inodePtrFromThis(),
        "pread failed during overlay file read");
  }

  buf->append(res.value());
  return BufVec{std::move(buf)};
}

size_t OverlayFileAccess::write(
    FileInode& inode,
    const struct iovec* iov,
    size_t iovcnt,
    off_t off) {
  auto entry = getEntryForInode(inode.getNodeId());

  auto xfer = entry->file.pwritev(iov, iovcnt, off + FsOverlay::kHeaderLength);
  if (xfer.hasError()) {
    throw InodeError(
        xfer.error(),
        inode.inodePtrFromThis(),
        "pwritev failed during file write");
  }
  auto info = entry->info.wlock();
  info->invalidateMetadata();

  return xfer.value();
}

void OverlayFileAccess::truncate(FileInode& inode, off_t size) {
  auto entry = getEntryForInode(inode.getNodeId());
  auto result = entry->file.ftruncate(size + FsOverlay::kHeaderLength);
  if (result.hasError()) {
    throw InodeError(
        result.error(),
        inode.inodePtrFromThis(),
        "unable to ftruncate overlay file");
  }

  auto info = entry->info.wlock();
  info->invalidateMetadata();
}

void OverlayFileAccess::fsync(FileInode& inode, bool datasync) {
  // TODO: If the inode is not currently in cache, we could avoid calling fsync.
  // That said, close() does not ensure data is synced, so it's safest to
  // reopen.
  auto entry = getEntryForInode(inode.getNodeId());
  auto result = datasync ? entry->file.fdatasync() : entry->file.fsync();
  if (result.hasError()) {
    throw InodeError(
        result.error(),
        inode.inodePtrFromThis(),
        "unable to fsync overlay file");
  }
}

OverlayFileAccess::EntryPtr OverlayFileAccess::getEntryForInode(
    InodeNumber ino) {
  {
    auto state = state_.wlock();
    auto iter = state->entries.find(ino);
    if (iter != state->entries.end()) {
      return iter->second;
    }
  }

  // No entry found. Open one while the lock is not held.
  // TODO: A possible future optimization here is, if a SHA-1 is known when
  // the blob is evicted, write it into an xattr when the blob is closed. When
  // reopened, if the xattr exists, read it back out (and clear).
  auto entry = std::make_shared<Entry>(
      overlay_->openFileNoVerify(ino), std::nullopt, std::nullopt);

  {
    auto state = state_.wlock();
    state->entries.set(ino, entry);
  }

  return entry;
}

} // namespace eden
} // namespace facebook
