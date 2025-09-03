/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/File.h>
#include <folly/Range.h>
#include <optional>

#include "eden/common/utils/FileOffset.h"
#include "eden/common/utils/ImmediateFuture.h"
#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/inodes/FileContentStore.h"
#include "eden/fs/inodes/InodeCatalog.h"
#include "eden/fs/inodes/lmdbcatalog/LMDBStoreInterface.h"
#include "eden/fs/inodes/overlay/OverlayChecker.h"
#include "eden/fs/model/Tree.h"

namespace folly {
class File;
}

namespace facebook::eden {

class EdenConfig;
namespace overlay {
class OverlayDir;
}
struct InodeNumber;
class StructuredLogger;

/**
 * Class to manage the on disk data.
 */
class LMDBFileContentStore : public FileContentStore {
 public:
  explicit LMDBFileContentStore(
      AbsolutePathPiece path,
      std::shared_ptr<StructuredLogger> logger);

  explicit LMDBFileContentStore(std::unique_ptr<LMDBDatabase> store)
      : store_(std::move(store)) {}

  /**
   * Initialize the LMDBFileContentStore
   */
  bool initialize(bool createIfNonExisting, bool bypassLockFile = false)
      override;

  /**
   * Gracefully shutdown the file content store.
   */
  void close() override;

  /**
   * Was FileContentStore initialized - i.e., is cleanup (close) necessary.
   */
  bool initialized() const override;

  /**
   * call statfs(2) on the filesystem in which the overlay is located
   */
  struct statfs statFs() const override;

  /**
   * Helper function that creates an overlay file for a new FileInode.
   */
  std::variant<folly::File, InodeNumber> createOverlayFile(
      InodeNumber inodeNumber,
      folly::ByteRange contents) override;

  /**
   * Helper function to write an overlay file for a FileInode with existing
   * contents.
   */
  std::variant<folly::File, InodeNumber> createOverlayFile(
      InodeNumber inodeNumber,
      const folly::IOBuf& contents) override;

  /**
   * Returns the overlay file contents for the given InodeNumber.
   */
  std::string readOverlayFile(InodeNumber inodeNumber);

  /**
   * Remove the overlay directory data associated with the passed InodeNumber.
   */
  void removeOverlayFile(InodeNumber inodeNumber) override;

  /**
   * Same as openFileNoVerify since LMDB doesn't need to verify the header.
   */
  std::variant<folly::File, InodeNumber> openFile(
      InodeNumber inodeNumber,
      folly::StringPiece /**/) override;

  /**
   * Open an existing overlay file without verifying the header.
   */
  std::variant<folly::File, InodeNumber> openFileNoVerify(
      InodeNumber inodeNumber) override;

  bool hasOverlayFile(InodeNumber inodeNumber) override;

  // OverlayFile API

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
  FileOffset allocateOverlayFile(
      InodeNumber inodeNumber,
      FileOffset offset,
      FileOffset length);

  /**
   * Writes up to `n` bytes from the buffer starting at buf to the blob for a
   * given InodeNumberat offset `offset`.
   *
   * Returns the number of bytes written on success, -1 on error or if the blob
   * does not exist.
   */
  FileOffset pwriteOverlayFile(
      InodeNumber inodeNumber,
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
  FileOffset preadOverlayFile(
      InodeNumber inodeNumber,
      void* buf,
      size_t n,
      FileOffset offset);

  /**
   * Returns the size of the blob for a given InodeNumber.
   *
   * Returns the size of the blob on success, -1 on error or if the blob does
   * not exist.
   */
  FileOffset getOverlayFileSize(InodeNumber inodeNumber);

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
  FileOffset truncateOverlayFile(InodeNumber inodeNumber, FileOffset length);

 private:
  void validateExistingOverlay(int infoFD);

  friend class LMDBInodeCatalog;
  const AbsolutePath path_;
  LMDBStoreInterface store_;
  bool initialized_ = false;
  folly::File infoFile_;
};

} // namespace facebook::eden
