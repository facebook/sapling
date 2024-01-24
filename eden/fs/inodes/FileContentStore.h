/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>
#include <variant>

#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/utils/PathFuncs.h"

#if defined(__APPLE__)
#include <sys/mount.h>
#include <sys/param.h>
#elif defined(__linux__)
#include <sys/vfs.h>
#endif

namespace folly {
class File;
class IOBuf;
} // namespace folly

namespace facebook::eden {

/**
 * Interface to manage materalized file data.
 */
class FileContentStore {
 public:
  FileContentStore() = default;

  virtual ~FileContentStore() = default;

  FileContentStore(const FileContentStore&) = delete;
  FileContentStore& operator=(const FileContentStore&) = delete;
  FileContentStore(FileContentStore&&) = delete;
  FileContentStore&& operator=(FileContentStore&&) = delete;

  virtual bool initialize(
      bool createIfNonExisting,
      bool bypassLockFile = false) = 0;

  /**
   * Gracefully shutdown the file content store.
   */
  virtual void close() = 0;

  /**
   * Was FileContentStore initialized - i.e., is cleanup (close) necessary.
   */
  virtual bool initialized() const = 0;

  /**
   * Remove the overlay data associated with the passed InodeNumber.
   */
  virtual void removeOverlayFile(InodeNumber inodeNumber) = 0;

  /**
   * Returns true if the overlay has data associated with the passed
   * InodeNumber.
   */
  virtual bool hasOverlayFile(InodeNumber inodeNumber) = 0;

#ifndef _WIN32
  /**
   * call statfs(2) on the filesystem in which the overlay is located
   */
  virtual struct statfs statFs() const = 0;

  /**
   * Helper function that opens an existing overlay file,
   * checks if the file has valid header, and returns the file.
   */
  virtual std::variant<folly::File, InodeNumber> openFile(
      InodeNumber inodeNumber,
      folly::StringPiece headerId) = 0;

  /**
   * Open an existing overlay file without verifying the header.
   */
  virtual std::variant<folly::File, InodeNumber> openFileNoVerify(
      InodeNumber inodeNumber) = 0;

  /**
   * Helper function that creates an overlay file for a new FileInode.
   */
  virtual std::variant<folly::File, InodeNumber> createOverlayFile(
      InodeNumber inodeNumber,
      folly::ByteRange contents) = 0;

  /**
   * Helper function to write an overlay file for a FileInode with existing
   * contents.
   */
  virtual std::variant<folly::File, InodeNumber> createOverlayFile(
      InodeNumber inodeNumber,
      const folly::IOBuf& contents) = 0;
#endif
};

} // namespace facebook::eden
