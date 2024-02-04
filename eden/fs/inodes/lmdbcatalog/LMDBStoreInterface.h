/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <fmt/format.h>
#include <folly/portability/SysUio.h>
#include <gtest/gtest_prod.h>
#include <atomic>
#include <memory>

#include "eden/common/utils/FileOffset.h"
#include "eden/fs/lmdb/LMDBDatabase.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook::eden {
namespace overlay {
class OverlayDir;
class OverlayEntry;
} // namespace overlay
struct InodeNumber;
class StructuredLogger;

class LMDBStoreInterfaceNonEmptyError : public std::exception {
 public:
  explicit LMDBStoreInterfaceNonEmptyError(std::string&& str)
      : message_(folly::to<std::string>(
            "Attempting to operate on non-empty directory: ",
            str)) {}

  const char* what() const noexcept override {
    return message_.c_str();
  }

 private:
  std::string message_;
};

/**
 * An interface into LMDB for use in the Overlay
 */
class LMDBStoreInterface {
 public:
  explicit LMDBStoreInterface(
      AbsolutePathPiece dir,
      std::shared_ptr<StructuredLogger> logger);

  explicit LMDBStoreInterface(std::unique_ptr<LMDBDatabase> db);

  ~LMDBStoreInterface() = default;

  LMDBStoreInterface(const LMDBStoreInterface&) = delete;
  LMDBStoreInterface& operator=(const LMDBStoreInterface&) = delete;
  LMDBStoreInterface(LMDBStoreInterface&& other) = delete;
  LMDBStoreInterface& operator=(LMDBStoreInterface&& other) = delete;

  void close();

  /**
   * Method for testing purposes to take the database to pass to the constructor
   */
  std::unique_ptr<LMDBDatabase> takeDatabase();

  void maintenance() {
    db_->checkpoint();
  }

  /**
   * Load the internal counters (next inode number) based on data in the
   * storage.
   */
  InodeNumber loadCounters();

  /**
   * Retrieve next available inode number. Depends on loadCounters() being
   * called first (to initialize nextInode_).
   */
  InodeNumber nextInodeNumber();

  /**
   * Get all parent inode numbers (keys) from the table
   */
  std::vector<InodeNumber> getAllParentInodeNumbers();

  /**
   * Save blob into storage
   */
  void saveBlob(InodeNumber inode, iovec* iov, size_t iovCount);

  /**
   * Save tree into storage
   */
  void saveTree(InodeNumber inode, std::string&& odir);

  /**
   * Load blob from storage
   */
  std::string loadBlob(InodeNumber inode);

  /**
   * Load tree from storage
   */
  overlay::OverlayDir loadTree(InodeNumber inode);

  /**
   * Remove the tree from the store and return it.
   */
  overlay::OverlayDir loadAndRemoveTree(InodeNumber inode);

  /**
   * Delete a tree from storage
   *
   * @throws if the tree being deleted is non-empty
   */
  void removeBlob(InodeNumber inode);

  /**
   * Delete a tree from storage
   *
   * @throws if the tree being deleted is non-empty
   */
  void removeTree(InodeNumber inode);

  /**
   * Check if the given inode number exists in the storage.
   */
  bool hasBlob(InodeNumber inode);

  /**
   * Check if the given inode number exists in the storage.
   */
  bool hasTree(InodeNumber inode);

  // The following funtions are provided to emulate the behavior of the
  // corresponding system calls. These are expected for use in the OverlayFile
  // class

  /**
   * Allocates the space within the range specified by offset and len. The blob
   * size will be increased if offset+len is greater than the existing size. Any
   * subregion within the range specified by offset and len that did not contain
   * data before the call will be initialized to zero. Any pre-existing data
   * will not be modified.
   *
   * Unlike fallocate(), this does not allocate in chunks, so extra data beyond
   * the requested size will not be allocated.
   *
   * Returns 0 on success, -1 on error or if the blob does not exist.
   */
  FileOffset
  allocateBlob(InodeNumber inode, FileOffset offset, FileOffset length);

  /**
   * Writes up to `n` bytes from the buffer starting at buf to the blob for a
   * given InodeNumber at offset `offset`.
   *
   * Returns the number of bytes written on success, -1 on error or if the blob
   * does not exist.
   */
  FileOffset pwriteBlob(
      InodeNumber inode,
      const struct iovec* iov,
      int iovcnt,
      FileOffset offset);

  /**
   * Reads up to `n` bytes from the blob for a given InodeNumber at offset
   * `offset` (from the start of the blob) into the buffer starting at buf.
   * Unlike pread(2), this will always read `n` bytes if available.
   *
   * Returns the number of bytes read on success, -1 on error or if the blob
   * does not exist.
   */
  FileOffset
  preadBlob(InodeNumber inode, void* buf, size_t n, FileOffset offset);

  /**
   * Returns the size of the blob for a given InodeNumber.
   *
   * Returns the size of the blob on success, -1 on error or if the blob does
   * not exist.
   */
  FileOffset getBlobSize(InodeNumber inode);

  /**
   * Truncates the blob for a given InodeNumber to a size of precisely length
   * bytes.
   *
   * If the blob previously was larger than this size, the extra data is lost.
   * If the blob previously was shorter, it is extended, and the extended part
   * reads as null bytes ('\0').
   *
   * Returns 0 on success, -1 on error or if the blob does not exist.
   */
  FileOffset truncateBlob(InodeNumber inode, FileOffset length);

 private:
  FRIEND_TEST(LMDBStoreInterfaceTest, testRecoverInodeEntryNumber);

  std::unique_ptr<LMDBDatabase> db_;

  std::atomic_uint64_t nextInode_{0};

  void removeData(InodeNumber inode);
  bool hasData(InodeNumber inode);
};
} // namespace facebook::eden
