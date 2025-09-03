/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/File.h>
#include <folly/Range.h>
#include <gtest/gtest_prod.h>
#include <array>
#include <optional>
#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/inodes/FileContentStore.h"
#include "eden/fs/inodes/InodeCatalog.h"
#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/inodes/overlay/gen-cpp2/overlay_types.h"
#ifdef __APPLE__
#include <sys/mount.h>
#include <sys/param.h>
#else
#include <sys/vfs.h>
#endif

namespace facebook::eden {

namespace overlay {
class OverlayDir;
}
class InodePathDev;

/**
 * Class to manage the on disk data.
 */
class FsFileContentStoreDev : public FileContentStore {
 public:
  explicit FsFileContentStoreDev(AbsolutePathPiece localDir)
      : localDir_{localDir} {}

  /**
   * Initialize the FileContentStore, acquire the "info" file lock and load the
   * nextInodeNumber. The "close" method should be used to release these
   * resources and persist the nextInodeNumber.
   *
   * Returns true if a new directory was created.
   */
  bool initialize(bool createIfNonExisting, bool bypassLockFile = false)
      override;

  /**
   * Gracefully shutdown the file content store.
   */
  void close() override;

  /**
   * Was FsFileContentStore initialized - i.e., is cleanup (close) necessary.
   */
  bool initialized() const override {
    return bool(infoFile_);
  }

  /**
   * This entrypoint is used by the OverlayChecker which needs the localDir
   * value but only has a pointer to the backing FsInodeCatalog object. In most
   * cases one should get the localDir by calling `Overlay::getLocalDir`
   * instead.
   */
  const AbsolutePath& getLocalDir() const {
    return localDir_;
  }

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
   * Remove the overlay directory data associated with the passed InodeNumber.
   */
  void removeOverlayFile(InodeNumber inodeNumber) override;

  /**
   * Helper function that opens an existing overlay file,
   * checks if the file has valid header, and returns the file.
   */
  std::variant<folly::File, InodeNumber> openFile(
      InodeNumber inodeNumber,
      folly::StringPiece headerId) override;

  /**
   * Open an existing overlay file without verifying the header.
   */
  std::variant<folly::File, InodeNumber> openFileNoVerify(
      InodeNumber inodeNumber) override;

  bool hasOverlayFile(InodeNumber inodeNumber) override;

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

  std::optional<fsck::InodeInfo> loadInodeInfo(InodeNumber number);

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
  friend class FsInodeCatalogDev;

  void initNewOverlay();

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
  void validateExistingOverlay(int infoFD);

  void saveNextInodeNumber(InodeNumber nextInodeNumber);

  /**
   * Creates header for the files stored in Overlay
   */
  static std::array<uint8_t, kHeaderLength> createHeader(
      folly::StringPiece identifier,
      uint32_t version);

  /**
   * Validates an entry's header.
   */
  static void validateHeader(
      InodeNumber inodeNumber,
      folly::StringPiece contents,
      folly::StringPiece headerId);

  /**
   * Get the path to the file for the given inode, relative to localDir.
   *
   * Returns a null-terminated InodePath value.
   */
  static InodePathDev getFilePath(InodeNumber inodeNumber);

  std::optional<overlay::OverlayDir> deserializeOverlayDir(
      InodeNumber inodeNumber);

  folly::File
  createOverlayFileImpl(InodeNumber inodeNumber, iovec* iov, size_t iovCount);

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

/**
 * FsInodeCatalog provides interfaces to manipulate the overlay. It stores the
 * overlay's file system attributes and is responsible for obtaining and
 * releasing its locks ("initOverlay" and "close" respectively).
 */
class FsInodeCatalogDev : public InodeCatalog {
 public:
  explicit FsInodeCatalogDev(FsFileContentStoreDev* core) : core_(core) {}

  bool supportsSemanticOperations() const override {
    return false;
  }

  std::vector<InodeNumber> getAllParentInodeNumbers() override {
    return {};
  }

  /**
   * Returns the next inode number to start at when allocating new inodes.
   * If the overlay was not shutdown cleanly by the previous user then
   * std::nullopt is returned.  In this case, the caller should re-scan
   * the overlay to check for issues and compute the next inode number.
   */
  std::optional<InodeNumber> initOverlay(
      bool createIfNonExisting,
      bool bypassLockFile = false) override;

  /**
   *  Gracefully, shutdown the overlay, persisting the overlay's
   * nextInodeNumber.
   */
  void close(std::optional<InodeNumber> nextInodeNumber) override;

  /**
   * Was FsInodeCatalog initialized - i.e., is cleanup (close) necessary.
   */
  bool initialized() const override;

  void saveOverlayDir(InodeNumber inodeNumber, overlay::OverlayDir&& odir)
      override;

  std::optional<overlay::OverlayDir> loadOverlayDir(
      InodeNumber inodeNumber) override;

  std::optional<overlay::OverlayDir> loadAndRemoveOverlayDir(
      InodeNumber inodeNumber) override;

  /**
   * Remove the overlay directory data associated with the passed InodeNumber.
   */
  void removeOverlayDir(InodeNumber inodeNumber) override;

  bool hasOverlayDir(InodeNumber inodeNumber) override;

  void maintenance() override {}

  std::optional<fsck::InodeInfo> loadInodeInfo(InodeNumber number) override;

 private:
  FsFileContentStoreDev* core_;
};

} // namespace facebook::eden
