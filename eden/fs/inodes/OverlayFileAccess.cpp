/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/inodes/OverlayFileAccess.h"

#include <folly/Expected.h>
#include <folly/Range.h>
#include <folly/logging/xlog.h>
#include <folly/portability/OpenSSL.h>

#include "eden/fs/digest/Blake3.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/InodeError.h"
#include "eden/fs/inodes/InodePtr.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/inodes/OverlayFile.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/model/Blob.h"

namespace facebook::eden {

constexpr size_t kHashingBufSize = 8192;

template <typename Hasher>
int hash(Hasher&& hasher, const OverlayFile& file) {
  FileOffset off = FsFileContentStore::kHeaderLength;
  uint8_t buf[kHashingBufSize];
  while (true) {
    const auto ret = file.preadNoInt(&buf, sizeof(buf), off);
    if (ret.hasError()) {
      return ret.error();
    }

    const auto len = ret.value();
    if (len == 0) {
      break;
    }

    hasher(buf, len);
    off += len;
  }

  return 0;
}

/*
 * OverlayFileAccess should be careful not to perform overlay IO operations
 * while its own state lock is held. Doing so serializes IO operations to the
 * overlay which impacts throughput under concurrent operations.
 */

void OverlayFileAccess::Entry::Info::invalidateMetadata() {
  ++version;
  size = std::nullopt;
  sha1 = std::nullopt;
  blake3 = std::nullopt;
}

OverlayFileAccess::State::State(size_t cacheSize) : entries{cacheSize} {
  if (cacheSize == 0) {
    throw std::range_error{"overlayFileCacheSize must be at least 1"};
  }
}

OverlayFileAccess::OverlayFileAccess(Overlay* overlay, size_t cacheSize)
    : overlay_{overlay}, state_{std::in_place, cacheSize} {}

OverlayFileAccess::~OverlayFileAccess() = default;

void OverlayFileAccess::createEmptyFile(
    InodeNumber ino,
    const std::optional<std::string>& maybeBlake3Key) {
  auto file = overlay_->createOverlayFile(ino, folly::ByteRange{});
  auto state = state_.wlock();
  XCHECK(!state->entries.exists(ino)) << fmt::format(
      "Cannot create overlay file {} when it's already open!", ino);

  // Computing the empty BLAKE3 hash for the given key
  auto blake3 = Blake3::create(maybeBlake3Key);
  Hash32 emptyBlake3;
  blake3.finalize(emptyBlake3.mutableBytes());

  state->entries.set(
      ino,
      std::make_shared<Entry>(
          std::move(file), size_t{0}, kEmptySha1, std::move(emptyBlake3)));
}

void OverlayFileAccess::createFile(
    InodeNumber ino,
    const Blob& blob,
    const std::optional<Hash20>& sha1,
    const std::optional<Hash32>& blake3) {
  auto file = overlay_->createOverlayFile(ino, blob.getContents());
  auto state = state_.wlock();
  XCHECK(!state->entries.exists(ino)) << fmt::format(
      "Cannot create overlay file {} when it's already open!", ino);
  state->entries.set(
      ino,
      std::make_shared<Entry>(std::move(file), blob.getSize(), sha1, blake3));
}

FileOffset OverlayFileAccess::getFileSize(FileInode& inode) {
  return getFileSize(inode.getNodeId(), &inode);
}

FileOffset OverlayFileAccess::getFileSize(InodeNumber ino, InodeBase* inode) {
  auto entry = getEntryForInode(ino);
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
        ret.error(),
        inode ? inode->inodePtrFromThis() : InodePtr{},
        "unable to fstat overlay file");
  }
  auto st = ret.value();
  if (st.st_size < static_cast<FileOffset>(FsFileContentStore::kHeaderLength)) {
    // Truncated overlay files can sometimes occur after a hard reboot
    // where the overlay file data was not flushed to disk before the
    // system powered off.
    XLOGF(
        ERR,
        "overlay file for {} is too short for header: size={}",
        ino,
        st.st_size);
    throw InodeError(
        EIO,
        inode ? inode->inodePtrFromThis() : InodePtr{},
        "corrupt overlay file");
  }

  auto size =
      st.st_size - static_cast<FileOffset>(FsFileContentStore::kHeaderLength);

  // Update the cache if the version still matches.
  auto info = entry->info.wlock();
  if (version == info->version) {
    info->size = size;
  }
  return size;
}

Hash20 OverlayFileAccess::getSha1(FileInode& inode) {
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
  if (auto r = hash(
          [&ctx](const auto* buf, auto len) { SHA1_Update(&ctx, buf, len); },
          entry->file);
      r != 0) {
    throw InodeError(
        r, inode.inodePtrFromThis(), "pread failed during SHA-1 calculation");
  }

  static_assert(Hash20::RAW_SIZE == SHA_DIGEST_LENGTH);
  Hash20 sha1;
  SHA1_Final(sha1.mutableBytes().begin(), &ctx);

  // Update the cache if the version still matches.
  auto info = entry->info.wlock();
  if (version == info->version) {
    info->sha1 = sha1;
  }
  return sha1;
}

Hash32 OverlayFileAccess::getBlake3(
    FileInode& inode,
    const std::optional<std::string>& maybeBlake3Key) {
  auto entry = getEntryForInode(inode.getNodeId());
  uint64_t version;
  {
    auto info = entry->info.rlock();
    if (info->blake3.has_value()) {
      return *info->blake3;
    }
    version = info->version;
  }

  auto blake3 = Blake3::create(maybeBlake3Key);
  if (auto r = hash(
          [&blake3](const auto* buf, auto len) { blake3.update(buf, len); },
          entry->file);
      r != 0) {
    throw InodeError(
        r, inode.inodePtrFromThis(), "pread failed during BLAKE3 calculation");
  }

  static_assert(Hash32::RAW_SIZE == BLAKE3_OUT_LEN);
  Hash32 hash;
  blake3.finalize(hash.mutableBytes());

  // Update the cache if the version still matches.
  auto info = entry->info.wlock();
  if (version == info->version) {
    info->blake3 = hash;
  }
  return hash;
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

  // Both LegacyInodeCatalog and LegacyDev use header files
  if (overlay_->getInodeCatalogType() == InodeCatalogType::Legacy ||
      overlay_->getInodeCatalogType() == InodeCatalogType::LegacyDev) {
    auto rc = entry->file.lseek(FsFileContentStore::kHeaderLength, SEEK_SET);
    if (rc.hasError()) {
      throw InodeError(
          rc.error(),
          inode.inodePtrFromThis(),
          "unable to seek in materialized FileInode");
    }
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

BufVec OverlayFileAccess::read(FileInode& inode, size_t size, FileOffset off) {
  auto entry = getEntryForInode(inode.getNodeId());

  auto buf = folly::IOBuf::createCombined(size);
  auto res = entry->file.preadNoInt(
      buf->writableBuffer(), size, off + FsFileContentStore::kHeaderLength);

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
    FileOffset off) {
  auto entry = getEntryForInode(inode.getNodeId());

  auto xfer =
      entry->file.pwritev(iov, iovcnt, off + FsFileContentStore::kHeaderLength);
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

void OverlayFileAccess::truncate(FileInode& inode, FileOffset size) {
  auto entry = getEntryForInode(inode.getNodeId());
  auto result = entry->file.ftruncate(size + FsFileContentStore::kHeaderLength);
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

void OverlayFileAccess::fallocate(
    FileInode& inode,
    uint64_t offset,
    uint64_t length) {
  auto entry = getEntryForInode(inode.getNodeId());
  auto result =
      entry->file.fallocate(offset, length + FsFileContentStore::kHeaderLength);
  if (result.hasError()) {
    throw InodeError(
        result.error(),
        inode.inodePtrFromThis(),
        "unable to fallocate overlay file");
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

} // namespace facebook::eden

#endif
