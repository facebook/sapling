/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/File.h>
#include <folly/Synchronized.h>
#include <folly/container/EvictingCacheMap.h>
#include <memory>
#include <optional>
#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/inodes/InodePtr.h"
#include "eden/fs/inodes/OverlayFile.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/utils/BufVec.h"

namespace facebook::eden {

class Blob;
class FileInode;
class Overlay;

/**
 * Provides a file handle caching layer between FileInode and the Overlay. Read
 * and write operations for different inodes can be interleaved, and the
 * OverlayFileAccess will keep a number of file handles open in LRU.
 */
class OverlayFileAccess {
 public:
  explicit OverlayFileAccess(Overlay* overlay, size_t cacheSize);
  ~OverlayFileAccess();

  /**
   * Creates a new empty file in the overlay.
   *
   * The caller must verify the overlay file does not already exist. Calls to
   * any other OverlayFileAccess functions for this inode must occur after
   * createEmptyFile returns.
   */
  void createEmptyFile(
      InodeNumber ino,
      const std::optional<std::string>& maybeBlake3Key);

  /**
   * Creates a new file in the overlay populated with the contents of the given
   * blob. If a sha1 is given, it is cached in memory.
   *
   * The caller must verify the overlay file does not already exist. Calls to
   * any other OverlayFileAccess functions for this inode must occur after
   * createFile returns.
   */
  void createFile(
      InodeNumber ino,
      const Blob& blob,
      const std::optional<Hash20>& sha1,
      const std::optional<Hash32>& blake3);

  /**
   * Return the size of the overlay file at the given inode number. The result
   * will never be negative.
   *
   * The inode parameter is used for error messages when the underlying overlay
   * data has been corrupted.
   */
  FileOffset getFileSize(FileInode& inode);
  FileOffset getFileSize(InodeNumber ino, InodeBase* inode);

  /**
   * Returns the SHA-1 hash of the file contents for the given inode number.
   */
  Hash20 getSha1(FileInode& inode);

  /**
   * Returns the BLAKE3 hash of the file contents for the given inode number.
   */
  Hash32 getBlake3(
      FileInode& inode,
      const std::optional<std::string>& maybeBlake3Key);

  /**
   * Reads the entire file's contents into memory and returns it.
   */
  std::string readAllContents(FileInode& inode);

  /**
   * Reads a range from the file. At EOF, may return a BufVec smaller than the
   * requested size.
   */
  BufVec read(FileInode& inode, size_t size, FileOffset off);

  /**
   * Writes data into the file at the specified offset. Returns the number of
   * bytes written.
   */
  size_t write(
      FileInode& inode,
      const struct iovec* iov,
      size_t iovcnt,
      FileOffset off);

  /**
   * Sets the size of the file in the overlay.
   */
  void truncate(FileInode& inode, FileOffset size = 0);

  /**
   * Flushes inode data to disk.
   *
   * If datasync is true, only the user data should be flushed, not the
   * metadata. It corresponds to datasync parameter to fuse_lowlevel_ops::fsync.
   */
  void fsync(FileInode& inode, bool datasync);

  /**
   * Call fallocate(mode=0) or posix_fallocate on the backing overlay storage.
   */
  void fallocate(FileInode& inode, uint64_t offset, uint64_t size);

 private:
  /*
   * OverlayFileAccess can be accessed concurrently. There are two types of data
   * to serialize under locks: the LRU cache (State::entries) and the per-inode,
   * in-memory size and SHA-1 caches.
   *
   * A lock around the size and hash is necessary because they can be read and
   * updated by concurrent getFileSize and getSha1 calls. (And write() and
   * truncate() clear them.)
   *
   * In addition, these locks should not be held while performing IO on the
   * overlay files - it's beneficial to expose maximum concurrency to the
   * backing filesystem.
   *
   * To avoid poisoning the SHA-1 and size caches when getSize and getSha1 are
   * concurrent with write or truncate, a version number is incremented on every
   * modification to an entry's file, and checked before writing the cached
   * value back.
   */

  struct Entry {
    Entry(
        OverlayFile f,
        std::optional<size_t> s,
        const std::optional<Hash20>& sha1,
        const std::optional<Hash32>& blake3 = std::nullopt)
        : file{std::move(f)}, info{std::in_place, s, sha1, blake3} {}

    struct Info {
      Info(
          std::optional<size_t> s,
          const std::optional<Hash20>& sha1,
          const std::optional<Hash32>& blake3)
          : size{s}, sha1{sha1}, blake3{blake3} {}

      void invalidateMetadata();

      std::optional<size_t> size;
      std::optional<Hash20> sha1;
      std::optional<Hash32> blake3;
      uint64_t version{0};
    };

    const OverlayFile file;
    folly::Synchronized<Info> info;
  };

  using EntryPtr = std::shared_ptr<Entry>;

  struct State {
    explicit State(size_t cacheSize);

    folly::EvictingCacheMap<InodeNumber, EntryPtr> entries;
  };

  using LockedStatePtr = folly::Synchronized<State>::LockedPtr;

  /**
   * Looks up an entry for the given inode. If the entry exists, it is returned.
   * Otherwise, one is loaded (and an old entry evicted if the cache is full).
   */
  EntryPtr getEntryForInode(InodeNumber);

  Overlay* overlay_ = nullptr;
  folly::Synchronized<State> state_;
};

} // namespace facebook::eden
