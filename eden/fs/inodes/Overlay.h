/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once
#include <folly/File.h>
#include <folly/Range.h>
#include <folly/futures/Promise.h>
#include <gtest/gtest_prod.h>
#include <array>
#include <condition_variable>
#include <optional>
#include <thread>
#include "eden/fs/fuse/FuseTypes.h"
#include "eden/fs/inodes/InodeTimestamps.h"
#include "eden/fs/inodes/gen-cpp2/overlay_types.h"
#include "eden/fs/utils/DirType.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook {
namespace eden {

namespace overlay {
class OverlayDir;
}

struct DirContents;
class InodeMap;
struct InodeMetadata;
template <typename T>
class InodeTable;
using InodeMetadataTable = InodeTable<InodeMetadata>;
struct SerializedInodeMap;

/** Manages the write overlay storage area.
 *
 * The overlay is where we store files that are not yet part of a snapshot.
 *
 * The contents of this storage layer are overlaid on top of the object store
 * snapshot that is active in a given mount point.
 *
 * There is one overlay area associated with each eden client instance.
 *
 * We use the Overlay to manage mutating the structure of the checkout;
 * each time we create or delete a directory entry, we do so through
 * the overlay class.
 *
 * The Overlay class keeps track of the mutated tree; if we mutate some
 * file "foo/bar/baz" then the Overlay records metadata about the list
 * of files in the root, the list of files in "foo", the list of files in
 * "foo/bar" and finally materializes "foo/bar/baz".
 */
class Overlay {
 public:
  class InodePath;

  explicit Overlay(AbsolutePathPiece localDir);
  ~Overlay();

  Overlay(const Overlay&) = delete;
  Overlay(Overlay&&) = delete;
  Overlay& operator=(const Overlay&) = delete;
  Overlay& operator=(Overlay&&) = delete;

  /**
   * Closes the overlay. It is undefined behavior to access the
   * InodeMetadataTable concurrently or call any other Overlay method
   * concurrently with or after calling close(). The Overlay will try to detect
   * this with assertions but cannot always detect concurrent access.
   *
   * Returns the next available InodeNumber to be passed to any process taking
   * over an Eden mount.
   */
  void close();

  /**
   * Returns true if the next inode number was initialized, either upon
   * construction by loading the file left by a cleanly-closed Overlay, or by
   * calling scanForNextInodeNumber().
   */
  bool hasInitializedNextInodeNumber() const;

  /**
   * Scans the Overlay for all inode numbers currently in use and sets the next
   * inode number to the maximum plus one. Either this or setNextInodeNumber
   * should be called when opening a mount point to ensure that any future
   * allocated inode numbers are always greater than those already tracked in
   * the overlay.
   *
   * Returns the maximum existing inode number.
   */
  InodeNumber scanForNextInodeNumber();

  /**
   * allocateInodeNumber() should only be called by TreeInode.
   *
   * This can be called:
   * - To allocate an inode number for an existing tree entry that does not
   *   need to be loaded yet.
   * - To allocate an inode number for a brand new inode being created by
   *   TreeInode::create() or TreeInode::mkdir().  In this case
   *   inodeCreated() should be called immediately afterwards to register the
   *   new child Inode object.
   *
   * It is illegal to call allocateInodeNumber prior to
   * setNextInodeNumber or scanForNextInodeNumber.
   *
   * TODO: It would be easy to extend this function to allocate a range of
   * inode values in one atomic operation.
   */
  InodeNumber allocateInodeNumber();

  /**
   * Returns an InodeMetadataTable for accessing and storing inode metadata.
   * Owned by the Overlay so records can be removed when the Overlay discovers
   * it no longer needs data for an inode or its children.
   */
  InodeMetadataTable* getInodeMetadataTable() const {
    return inodeMetadataTable_.get();
  }

  void saveOverlayDir(
      InodeNumber inodeNumber,
      const DirContents& dir,
      const InodeTimestamps& timestamps);

  std::optional<std::pair<DirContents, InodeTimestamps>> loadOverlayDir(
      InodeNumber inodeNumber);

  void removeOverlayData(InodeNumber inodeNumber);

  /**
   * Remove the overlay data for the given tree inode and recursively remove
   * everything beneath it too.
   *
   * Must only be called on trees.
   */
  void recursivelyRemoveOverlayData(InodeNumber inodeNumber);

  /**
   * Returns a future that completes once all previously-issued async
   * operations, namely recursivelyRemoveOverlayData, finish.
   */
  folly::Future<folly::Unit> flushPendingAsync();

  bool hasOverlayData(InodeNumber inodeNumber);

  /**
   * Helper function that opens an existing overlay file,
   * checks if the file has valid header
   * populates st_atim, st_mtim, st_ctim and returns the file.
   */
  folly::File openFile(
      InodeNumber inodeNumber,
      folly::StringPiece headerId,
      InodeTimestamps& timestamps);

  /**
   * Open an existing overlay file without verifying the header.
   */
  folly::File openFileNoVerify(InodeNumber inodeNumber);

  /**
   * Helper function that creates an overlay file for a new FileInode.
   */
  folly::File createOverlayFile(
      InodeNumber inodeNumber,
      const InodeTimestamps& timestamps,
      folly::ByteRange contents);

  /**
   * Helper function to write an overlay file for a FileInode with existing
   * contents.
   */
  folly::File createOverlayFile(
      InodeNumber inodeNumber,
      const InodeTimestamps& timestamps,
      const folly::IOBuf& contents);

  /**
   * Constants for an header in overlay file.
   */
  static constexpr folly::StringPiece kHeaderIdentifierDir{"OVDR"};
  static constexpr folly::StringPiece kHeaderIdentifierFile{"OVFL"};
  static constexpr uint32_t kHeaderVersion = 1;
  static constexpr size_t kHeaderLength = 64;

  /**
   * The number of digits required for a decimal representation of an
   * inode number.
   */
  static constexpr size_t kMaxDecimalInodeNumberLength = 20;

 private:
  FRIEND_TEST(OverlayTest, getFilePath);
  friend class RawOverlayTest;

  /**
   * A request for the background GC thread.  There are two types of requests:
   * recursively forget data underneath an given directory, or complete a
   * promise.  The latter is used for synchronization with the GC thread,
   * primarily in unit tests.
   *
   * If additional request types are added in the future, consider renaming to
   * AsyncRequest.  However, recursive collection of forgotten inode numbers
   * is the only operation that can be made async while preserving our
   * durability goals.
   */
  struct GCRequest {
    GCRequest() {}
    explicit GCRequest(overlay::OverlayDir&& d) : dir{std::move(d)} {}
    explicit GCRequest(folly::Promise<folly::Unit> p) : flush{std::move(p)} {}

    overlay::OverlayDir dir;
    // Iff set, this is a flush request.
    std::optional<folly::Promise<folly::Unit>> flush;
  };

  struct GCQueue {
    bool stop = false;
    std::vector<GCRequest> queue;
  };

  void initOverlay();
  void tryLoadNextInodeNumber();
  void saveNextInodeNumber();
  void readExistingOverlay(int infoFD);
  void initNewOverlay();
  void ensureTmpDirectoryIsCreated();

  std::optional<overlay::OverlayDir> deserializeOverlayDir(
      InodeNumber inodeNumber,
      InodeTimestamps& timeStamps) const;

  /**
   * Creates header for the files stored in Overlay
   */
  static std::array<uint8_t, kHeaderLength> createHeader(
      folly::StringPiece identifier,
      uint32_t version,
      const InodeTimestamps& timestamps);

  folly::File
  createOverlayFileImpl(InodeNumber inodeNumber, iovec* iov, size_t iovCount);

  /**
   * Get the path to the file for the given inode, relative to localDir_.
   *
   * Returns a null-terminated InodePath value.
   */
  static InodePath getFilePath(InodeNumber inodeNumber);

  /**
   * Parses, validates and reads Timestamps from the header.
   */
  static void parseHeader(
      folly::StringPiece header,
      folly::StringPiece headerId,
      InodeTimestamps& timeStamps);

  void gcThread() noexcept;
  void handleGCRequest(GCRequest& request);

  /** path to ".eden/CLIENT/local" */
  const AbsolutePath localDir_;

  /**
   * The next inode number to allocate.  Zero indicates that neither
   * initializeFromTakeover nor getMaxRecordedInode have been called.
   *
   * This value will never be 1.
   */
  std::atomic<uint64_t> nextInodeNumber_{0};

  /**
   * An open file descriptor to the overlay info file.
   *
   * This is primarily used to hold a lock on the overlay for as long as we are
   * using it.  We want to ensure that only one eden process accesses the
   * Overlay directory at a time.
   */
  folly::File infoFile_;

  /**
   * An open file to the overlay directory.
   *
   * We maintain this so we can use openat(), unlinkat(), etc.
   */
  folly::File dirFile_;

  /**
   * Disk-backed mapping from inode number to InodeMetadata.
   * Defined below infoFile_ because it acquires its own file lock, which should
   * be released first during shutdown.
   */
  std::unique_ptr<InodeMetadataTable> inodeMetadataTable_;

  /**
   * Thread which recursively removes entries from the overlay underneath the
   * trees added to gcQueue_.
   */
  std::thread gcThread_;
  folly::Synchronized<GCQueue, std::mutex> gcQueue_;
  std::condition_variable gcCondVar_;
};

class Overlay::InodePath {
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
      2 + 1 + Overlay::kMaxDecimalInodeNumberLength + 1;

  const char* c_str() const noexcept;
  /* implicit */ operator RelativePathPiece() const noexcept;

  std::array<char, kMaxPathLength>& rawData() noexcept;

 private:
  std::array<char, kMaxPathLength> path_;
};

} // namespace eden
} // namespace facebook
