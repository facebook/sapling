/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/File.h>
#include <folly/Synchronized.h>
#include <folly/container/EvictingCacheMap.h>
#include <memory>
#include "eden/fs/fuse/BufVec.h"
#include "eden/fs/fuse/InodeNumber.h"
#include "eden/fs/inodes/OverlayFile.h"
#include "eden/fs/model/Hash.h"

namespace facebook {
namespace eden {

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
  explicit OverlayFileAccess(Overlay* overlay);
  ~OverlayFileAccess();

  /**
   * Creates a new empty file in the overlay.
   *
   * The caller must verify the overlay file does not already exist. Calls to
   * any other OverlayFileAccess functions for this inode must occur after
   * createEmptyFile returns.
   */
  void createEmptyFile(InodeNumber ino);

  /**
   * Creates a new file in the overlay populated with the contents of the given
   * blob. If a sha1 is given, it is cached in memory.
   *
   * The caller must verify the overlay file does not already exist. Calls to
   * any other OverlayFileAccess functions for this inode must occur after
   * createEmptyFile returns.
   */
  void createFile(
      InodeNumber ino,
      const Blob& blob,
      const std::optional<Hash>& sha1);

  /**
   * Return the size of the overlay file at the given inode number. The result
   * will never be negative.
   *
   * The inode parameter is used for error messages when the underlying overlay
   * data has been corrupted.
   */
  off_t getFileSize(InodeNumber ino, FileInode& inode);

  /**
   * Returns the SHA-1 hash of the file contents for the given inode number.
   */
  Hash getSha1(InodeNumber ino);

  /**
   * Reads the entire file's contents into memory and returns it.
   */
  std::string readAllContents(InodeNumber ino);

  /**
   * Reads a range from the file. At EOF, may return a BufVec smaller than the
   * requested size.
   */
  BufVec read(InodeNumber ino, size_t size, off_t off);

  /**
   * Writes data into the file at the specified offset. Returns the number of
   * bytes written.
   */
  size_t
  write(InodeNumber ino, const struct iovec* iov, size_t iovcnt, off_t off);

  /**
   * Sets the size of the file in the overlay.
   */
  void truncate(InodeNumber ino, off_t size = 0);

  /**
   * If datasync is true, only the user data should be flushed, not the
   * metadata. It corresponds to datasync parameter to fuse_lowlevel_ops::fsync.
   */
  void fsync(InodeNumber ino, bool datasync);

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
    Entry(OverlayFile f, std::optional<size_t> s, const std::optional<Hash>& h)
        : file{std::move(f)}, info{folly::in_place, s, h} {}

    struct Info {
      Info(std::optional<size_t> s, const std::optional<Hash>& h)
          : size{s}, sha1{h} {}

      void invalidateMetadata();

      std::optional<size_t> size;
      std::optional<Hash> sha1;
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

} // namespace eden
} // namespace facebook
