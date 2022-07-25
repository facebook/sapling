/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include <folly/File.h>
#include <folly/Range.h>
#include <folly/futures/Future.h>
#include <folly/futures/Promise.h>
#include <folly/synchronization/Baton.h>
#include <array>
#include <atomic>
#include <condition_variable>
#include <optional>
#include <thread>
#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/inodes/fsoverlay/OverlayChecker.h"
#include "eden/fs/inodes/overlay/gen-cpp2/overlay_types.h"
#include "eden/fs/telemetry/StructuredLogger.h"
#include "eden/fs/utils/CaseSensitivity.h"
#include "eden/fs/utils/DirType.h"
#include "eden/fs/utils/ImmediateFuture.h"
#include "eden/fs/utils/PathFuncs.h"

#ifndef _WIN32
#include "eden/fs/inodes/fsoverlay/FsOverlay.h"
#endif

namespace facebook::eden {

namespace overlay {
class OverlayDir;
}

struct DirContents;
class InodeMap;
class SerializedInodeMap;
class IOverlay;
class DirEntry;
class EdenConfig;

#ifndef _WIN32
struct InodeMetadata;
template <typename T>
class InodeTable;
using InodeMetadataTable = InodeTable<InodeMetadata>;
class OverlayFile;
#endif

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
class Overlay : public std::enable_shared_from_this<Overlay> {
 public:
  enum class OverlayType : uint8_t {
    Legacy = 0,
    Tree = 1,
    TreeInMemory = 2,
    TreeSynchronousOff = 3,
    TreeBuffered = 4,
  };

  /**
   * Create a new Overlay object.
   *
   * The caller must call initialize() after creating the Overlay and wait for
   * it to succeed before using any other methods.
   */
  static std::shared_ptr<Overlay> create(
      AbsolutePathPiece localDir,
      CaseSensitivity caseSensitive,
      OverlayType overlayType,
      std::shared_ptr<StructuredLogger> logger,
      const EdenConfig& config);

  ~Overlay();

  Overlay(const Overlay&) = delete;
  Overlay(Overlay&&) = delete;
  Overlay& operator=(const Overlay&) = delete;
  Overlay& operator=(Overlay&&) = delete;

  /**
   * Initialize the overlay.
   *
   * This must be called after the Overlay constructor, before performing
   * operations on the overlay.
   *
   * This may be a slow operation and may perform significant amounts of
   * disk I/O.
   *
   * The initialization operation may include:
   * - Acquiring a lock to ensure no other processes are accessing the on-disk
   *   overlay state
   * - Creating the initial on-disk overlay data structures if necessary.
   * - Verifying and fixing the on-disk data if the Overlay was not shut down
   *   cleanly the last time it was opened.
   * - Upgrading the on-disk data from older formats if the Overlay was created
   *   by an older version of the software.
   */
  FOLLY_NODISCARD folly::SemiFuture<folly::Unit> initialize(
      std::optional<AbsolutePath> mountPath = std::nullopt,
      OverlayChecker::ProgressCallback&& progressCallback = [](auto) {},
      OverlayChecker::LookupCallback&& lookupCallback =
          [](auto) {
            return makeImmediateFuture<OverlayChecker::LookupCallbackValue>(
                std::runtime_error("no lookup callback"));
          });

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
   * Returns true if the Overlay has already been closed or is in the process of
   * closing. This function is primarily for debugging.
   */
  bool isClosed();

  /**
   * True if either a new verlay was created on disk or initialize() returned
   * after opening an overlay that had been cleanly shut down. False prior to
   * initialize() being called or if a consistency check was required.
   */
  bool hadCleanStartup() const {
    return hadCleanStartup_;
  }

  /**
   * Get the maximum inode number that has ever been allocated to an inode.
   */
  InodeNumber getMaxInodeNumber();

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
   * TODO: It would be easy to extend this function to allocate a range of
   * inode values in one atomic operation.
   */
  InodeNumber allocateInodeNumber();
#ifndef _WIN32

  /**
   * Returns an InodeMetadataTable for accessing and storing inode metadata.
   * Owned by the Overlay so records can be removed when the Overlay discovers
   * it no longer needs data for an inode or its children.
   */
  InodeMetadataTable* getInodeMetadataTable() const {
    return inodeMetadataTable_.get();
  }
#endif // !_WIN32

  void saveOverlayDir(InodeNumber inodeNumber, const DirContents& dir);

  /*
   * Load content of the directory from overlay. If the directory does not
   * exist, this function will return an empty `DirContents`.
   */
  DirContents loadOverlayDir(InodeNumber inodeNumber);

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

#ifndef _WIN32
  /**
   * Helper function that opens an existing overlay file,
   * checks if the file has valid header, and returns the file.
   */
  OverlayFile openFile(InodeNumber inodeNumber, folly::StringPiece headerId);

  /**
   * Open an existing overlay file without verifying the header.
   */
  OverlayFile openFileNoVerify(InodeNumber inodeNumber);

  /**
   * Helper function that creates an overlay file for a new FileInode.
   */
  OverlayFile createOverlayFile(
      InodeNumber inodeNumber,
      folly::ByteRange contents);

  /**
   * Helper function to write an overlay file for a FileInode with existing
   * contents.
   */
  OverlayFile createOverlayFile(
      InodeNumber inodeNumber,
      const folly::IOBuf& contents);

  /**
   * call statfs(2) on the filesystem in which the overlay is located
   */
  struct statfs statFs();
#endif // !_WIN32

  void addChild(
      InodeNumber parent,
      const std::pair<PathComponent, DirEntry>& childEntry,
      const DirContents& content);

  void removeChild(
      InodeNumber parent,
      PathComponentPiece childName,
      const DirContents& content);
  void removeChildren(InodeNumber parent, const DirContents& content);

  void renameChild(
      InodeNumber src,
      InodeNumber dst,
      PathComponentPiece srcName,
      PathComponentPiece dstName,
      const DirContents& srcContent,
      const DirContents& dstContent);

  /*
   * Some overlay type (TreeOverlay) requires periodic task to maintain their
   * underlying storage. This method is called periodically by an EdenServer
   * task in the configured interval.
   */
  void maintenance();

  /*
   * Returns a raw pointer to the backing overlay. This method should only be
   * used for testing.
   */
  IOverlay* getRawBackingOverlay() {
    return backingOverlay_.get();
  }

  overlay::OverlayDir serializeOverlayDir(
      InodeNumber inodeNumber,
      const DirContents& dir);

 private:
  explicit Overlay(
      AbsolutePathPiece localDir,
      CaseSensitivity caseSensitive,
      OverlayType overlayType,
      std::shared_ptr<StructuredLogger> logger,
      const EdenConfig& config);

  /**
   * A request for the background GC thread.  There are three types of
   * requests: recursively forget data underneath a given directory, perform
   * some maintenance of the overlay or complete a promise.  The latter is used
   * for synchronization with the GC thread, primarily in unit tests.
   *
   * If additional request types are added in the future, consider renaming to
   * AsyncRequest.  However, recursive collection of forgotten inode numbers
   * is the only operation that can be made async while preserving our
   * durability goals.
   */
  struct GCRequest {
    explicit GCRequest(overlay::OverlayDir&& d) : requestType{std::move(d)} {}

    using FlushRequest = folly::Promise<folly::Unit>;
    explicit GCRequest(FlushRequest p) : requestType{std::move(p)} {}

    struct MaintenanceRequest {};
    explicit GCRequest(MaintenanceRequest req) : requestType{std::move(req)} {}

    std::variant<MaintenanceRequest, overlay::OverlayDir, FlushRequest>
        requestType;
  };

  struct GCQueue {
    bool stop = false;
    std::vector<GCRequest> queue;
  };

  void initOverlay(
      std::optional<AbsolutePath> mountPath,
      const OverlayChecker::ProgressCallback& progressCallback,
      OverlayChecker::LookupCallback&& lookupCallback);
  void gcThread() noexcept;
  void handleGCRequest(GCRequest& request);

  // Serialize EdenFS overlay data structure into Thrift data structure
  overlay::OverlayEntry serializeOverlayEntry(const DirEntry& entry);

  bool tryIncOutstandingIORequests();
  void decOutstandingIORequests();
  void closeAndWaitForOutstandingIO();

  bool hadCleanStartup_{false};

  /**
   * The next inode number to allocate.  Zero indicates that neither
   * initializeFromTakeover nor getMaxRecordedInode have been called.
   *
   * This value will never be 1.
   */
  std::atomic<uint64_t> nextInodeNumber_{0};

  std::unique_ptr<IOverlay> backingOverlay_;

  /**
   * Indicates if the backing overlay supports semantic operations, see
   * `IOverlay::supportsSemanticOperations` for mor information.
   */
  bool supportsSemanticOperations_;

#ifndef _WIN32
  /**
   * Disk-backed mapping from inode number to InodeMetadata.
   * Defined below backingOverlay_ because it acquires its own file lock, which
   * should be released first during shutdown.
   */
  std::unique_ptr<InodeMetadataTable> inodeMetadataTable_;
#endif // !_WIN32

  /**
   * Wrapper around InodeMetadataTable::freeInode.
   *
   * This exist to limit the use of #ifdef to this function.
   */
  void freeInodeFromMetadataTable(InodeNumber ino);

  /**
   * Thread which recursively removes entries from the overlay underneath the
   * trees added to gcQueue_.
   */
  std::thread gcThread_;
  folly::Synchronized<GCQueue, std::mutex> gcQueue_;
  std::condition_variable gcCondVar_;

  /**
   * This uint64_t holds two values, a single bit on the MSB that
   * acts a boolean closed: True if the the Overlay has been closed with
   * calling setClosed(). When this is true, reads and writes will throw an
   * error instead of applying an overlay change or read. On the rest of the
   * bits, the actual number of outstanding IO requests is held. This has been
   * done in order to synchronize these two variables and treat checking if the
   * overlay is closed and incrementing the IO reference count as a single
   * atomic action.
   */
  mutable std::atomic<uint64_t> outstandingIORequests_{0};

  folly::Baton<> lastOutstandingRequestIsComplete_;
  CaseSensitivity caseSensitive_;

  std::shared_ptr<StructuredLogger> structuredLogger_;

  friend class IORequest;
};

constexpr Overlay::OverlayType kDefaultOverlayType = folly::kIsWindows
    ? Overlay::OverlayType::Tree
    : Overlay::OverlayType::Legacy;

/**
 * Used to reference count IO requests. In any place that there
 * is an overlay read or write, this struct should be constructed in order to
 * properly reference count and to properly deny overlay reads and
 * modifications in the case that the overlay is closed.
 */
class IORequest {
 public:
  explicit IORequest(Overlay* o) : overlay_{o} {
    if (!overlay_->tryIncOutstandingIORequests()) {
      throw std::system_error(
          EIO,
          std::generic_category(),
          folly::to<std::string>("cannot access overlay after it is closed"));
    }
  }

  ~IORequest() {
    overlay_->decOutstandingIORequests();
  }

 private:
  IORequest(IORequest&&) = delete;
  IORequest& operator=(IORequest&&) = delete;

  Overlay* const overlay_;
};

} // namespace facebook::eden
