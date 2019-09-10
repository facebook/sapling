/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include <folly/File.h>
#include <folly/Range.h>
#include <gtest/gtest_prod.h>
#include <array>
#include <condition_variable>
#include <optional>
#include "eden/fs/fuse/InodeNumber.h"
#include "eden/fs/inodes/overlay/gen-cpp2/overlay_types.h"
#include "eden/fs/utils/DirType.h"
#include "eden/fs/utils/PathFuncs.h"
#ifdef __APPLE__
#include <sys/mount.h>
#include <sys/param.h>
#else
#include <sys/vfs.h>
#endif

namespace facebook {
namespace eden {

namespace overlay {
class OverlayDir;
}
class InodePath;

/**
 * FsOverlay provides interfaces to manipulate the overlay. It stores the
 * overlay's file system attributes and is responsible for obtaining and
 * releasing its locks ("initOverlay" and "close" respectively).
 */
class FsOverlay {
 public:
  explicit FsOverlay(AbsolutePathPiece localDir) : localDir_{localDir} {}
  /**
   * Initialize the overlay, acquire the "info" file lock and load the
   * nextInodeNumber. The "close" method should be used to release these
   * resources and persist the nextInodeNumber.
   *
   * Returns the next inode number to start at when allocating new inodes.
   * If the overlay was not shutdown cleanly by the previous user then
   * std::nullopt is returned.  In this case, the caller should re-scan
   * the overlay to check for issues and compute the next inode number.
   */
  std::optional<InodeNumber> initOverlay(bool createIfNonExisting);
  /**
   *  Gracefully, shutdown the overlay, persisting the overlay's
   * nextInodeNumber.
   */
  void close(std::optional<InodeNumber> nextInodeNumber);
  /**
   * Was FsOverlay initialized - i.e., is cleanup (close) necessary.
   */
  bool initialized() const {
    return bool(infoFile_);
  }

  const AbsolutePath& getLocalDir() const {
    return localDir_;
  }

  /**
   * call statfs(2) on the filesystem in which the overlay is located
   */
  struct statfs statFs() const;

  /**
   *  Get the name of the subdirectory to use for the overlay data for the
   *  specified inode number.
   *
   *  We shard the inode files across the 256 subdirectories using the least
   *  significant byte.  Inode numbers are allocated in monotonically
   * increasing order, so this helps spread them out across the subdirectories.
   *
   * The shard directory paths are always exactly kShardDirPathLength bytes
   * long: the `subdirPath` argument must point to a buffer exactly
   * kShardDirPathLength bytes long.  This function will write to those bytes,
   * and no null terminator is included in the output.
   */
  static void formatSubdirPath(
      InodeNumber inodeNum,
      folly::MutableStringPiece subdirPath);

  /**
   * Format the subdir shard path given a shard ID from 0 to 255.
   *
   * The shard directory paths are always exactly kShardDirPathLength bytes
   * long: the `subdirPath` argument must point to a buffer exactly
   * kShardDirPathLength bytes long.  This function will write to those bytes,
   * and no null terminator is included in the output.
   */
  using ShardID = uint32_t;
  static void formatSubdirShardPath(
      ShardID shardID,
      folly::MutableStringPiece subdirPath);

  void initNewOverlay();

  void saveOverlayDir(InodeNumber inodeNumber, const overlay::OverlayDir& odir);

  std::optional<overlay::OverlayDir> loadOverlayDir(InodeNumber inodeNumber);

  void saveNextInodeNumber(InodeNumber nextInodeNumber);

  void writeNextInodeNumber(InodeNumber nextInodeNumber);

  /**
   * Return the next inode number from the kNextInodeNumberFile.  If the file
   * exists and contains a valid InodeNumber, that value is returned. If the
   * file does not exist, the optional will not have a value. If the file cannot
   * be opened or does not contain a valid InodeNumber, a SystemError is thrown.
   */
  std::optional<InodeNumber> tryLoadNextInodeNumber();

  /**
   * Validate an existing overlay's info file exists, is valid and contains the
   * correct version.
   */
  void readExistingOverlay(int infoFD);

  /**
   * Helper function that creates an overlay file for a new FileInode.
   */
  folly::File createOverlayFile(
      InodeNumber inodeNumber,
      folly::ByteRange contents);

  /**
   * Helper function to write an overlay file for a FileInode with existing
   * contents.
   */
  folly::File createOverlayFile(
      InodeNumber inodeNumber,
      const folly::IOBuf& contents);

  /**
   * Remove the overlay file associated with the passed InodeNumber.
   */
  void removeOverlayFile(InodeNumber inodeNumber);

  /**
   * Validates an entry's header.
   */
  static void validateHeader(
      InodeNumber inodeNumber,
      folly::StringPiece contents,
      folly::StringPiece headerId);

  /**
   * Helper function that opens an existing overlay file,
   * checks if the file has valid header, and returns the file.
   */
  folly::File openFile(InodeNumber inodeNumber, folly::StringPiece headerId);

  /**
   * Open an existing overlay file without verifying the header.
   */
  folly::File openFileNoVerify(InodeNumber inodeNumber);

  /**
   * Get the absolute path to a file to the overlay file for a given inode
   * number.
   *
   * Note that this method should not be needed in most normal circumstances:
   * most internal operation is done using getFilePath(), which returns relative
   * paths that are guaranteed to always fit in a fixed length.
   * getAbsoluteFilePath() is primarily intended for the fsck logic, where it is
   * sometimes useful to be able to get absolute paths to be able to move broken
   * files out of the overlay.
   */
  AbsolutePath getAbsoluteFilePath(InodeNumber inodeNumber) const;

  bool hasOverlayData(InodeNumber inodeNumber);

  static constexpr folly::StringPiece kMetadataFile{"metadata.table"};

  /**
   * Constants for an header in overlay file.
   */
  static constexpr folly::StringPiece kHeaderIdentifierDir{"OVDR"};
  static constexpr folly::StringPiece kHeaderIdentifierFile{"OVFL"};
  static constexpr uint32_t kHeaderVersion = 1;
  static constexpr size_t kHeaderLength = 64;
  static constexpr uint32_t kNumShards = 256;
  static constexpr size_t kShardDirPathLength = 2;

  /**
   * The number of digits required for a decimal representation of an
   * inode number.
   */
  static constexpr size_t kMaxDecimalInodeNumberLength = 20;

 private:
  FRIEND_TEST(OverlayTest, getFilePath);
  friend class RawOverlayTest;

  /**
   * Creates header for the files stored in Overlay
   */
  static std::array<uint8_t, kHeaderLength> createHeader(
      folly::StringPiece identifier,
      uint32_t version);

  /**
   * Get the path to the file for the given inode, relative to localDir.
   *
   * Returns a null-terminated InodePath value.
   */
  static InodePath getFilePath(InodeNumber inodeNumber);

  std::optional<overlay::OverlayDir> deserializeOverlayDir(
      InodeNumber inodeNumber);

  folly::File
  createOverlayFileImpl(InodeNumber inodeNumber, iovec* iov, size_t iovCount);

 private:
  /** Path to ".eden/CLIENT/local" */
  const AbsolutePath localDir_;

  /**
   * An open file descriptor to the overlay info file.
   *
   * This is primarily used to hold a lock on the overlay for as long as we
   * are using it.  We want to ensure that only one eden process accesses the
   * Overlay directory at a time.
   */
  folly::File infoFile_;

  /**
   * An open file to the overlay directory.
   *
   * We maintain this so we can use openat(), unlinkat(), etc.
   */
  folly::File dirFile_;
};

class InodePath {
 public:
  explicit InodePath() noexcept;

  /**
   * The maximum path length for the path to a file inside the overlay
   * directory.
   *
   * This is 2 bytes for the initial subdirectory name, 1 byte for the '/',
   * 20 bytes for the inode number, and 1 byte for a null terminator.
   */
  static constexpr size_t kMaxPathLength =
      2 + 1 + FsOverlay::kMaxDecimalInodeNumberLength + 1;

  const char* c_str() const noexcept;
  /* implicit */ operator RelativePathPiece() const noexcept;

  std::array<char, kMaxPathLength>& rawData() noexcept;

 private:
  std::array<char, kMaxPathLength> path_;
};

} // namespace eden
} // namespace facebook
