/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include <folly/File.h>
#include <folly/Function.h>
#include <folly/Range.h>
#include <folly/Synchronized.h>
#include <folly/container/F14Map.h>
#include <folly/futures/Future.h>
#include <folly/futures/Promise.h>
#include <folly/synchronization/Baton.h>
#include <folly/synchronization/LifoSem.h>
#include <array>
#include <atomic>
#include <condition_variable>
#include <optional>
#include <thread>
#include <unordered_map>

#include "eden/common/utils/CaseSensitivity.h"
#include "eden/common/utils/PathFuncs.h"
#include "eden/common/utils/RefPtr.h"
#include "eden/fs/config/InodeCatalogOptions.h"
#include "eden/fs/config/InodeCatalogType.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/inodes/overlay/OverlayChecker.h"
#include "eden/fs/inodes/overlay/gen-cpp2/overlay_types.h"
#include "eden/fs/telemetry/EdenFsEventsLogger.h"

#ifndef _WIN32
#include "eden/fs/inodes/fscatalog/EphemeralFsInodeCatalog.h"
#include "eden/fs/inodes/fscatalog/FsInodeCatalog.h"
#include "eden/fs/inodes/fscatalog_dev/FsInodeCatalogDev.h"
#endif

namespace facebook::eden {

namespace overlay {
class OverlayDir;
}

struct DirContents;
class InodeMap;
class SerializedInodeMap;
class InodeCatalog;
class FileContentStore;
class DirEntry;
class EdenConfig;
class EdenStats;

using EdenStatsPtr = RefPtr<EdenStats>;

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
  /**
   * Create a new Overlay object.
   *
   * The caller must call initialize() after creating the Overlay and wait for
   * it to succeed before using any other methods.
   */
  static std::shared_ptr<Overlay> create(
      AbsolutePathPiece localDir,
      CaseSensitivity caseSensitive,
      InodeCatalogType inodeCatalogType,
      InodeCatalogOptions inodeCatalogOptions,
      std::shared_ptr<EdenFsEventsLogger> logger,
      EdenStatsPtr stats,
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
  [[nodiscard]] folly::SemiFuture<folly::Unit> initialize(
      const std::shared_ptr<ReloadableConfig>& config,
      std::optional<AbsolutePath> mountPath = std::nullopt,
      OverlayChecker::ProgressCallback&& progressCallback = [](auto) {},
      InodeCatalog::LookupCallback&& lookupCallback =
          [](auto, auto) {
            return makeImmediateFuture<InodeCatalog::LookupCallbackValue>(
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
   * True if either a new overlay was created on disk or initialize() returned
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
   */
  InodeNumber allocateInodeNumber();

  /**
   * Allocate a contiguous range of inode numbers.
   *
   * Returns the first inode number in the range. The allocated range is
   * [returned, returned + count). Uses a single atomic operation instead of
   * count separate increments.
   */
  InodeNumber allocateInodeNumbers(uint64_t count);

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

  /**
   * Save a directory to the overlay.
   *
   * When isMaterialized is false, the directory contents match source control
   * and can be reconstructed from the backing store. In this case, the write
   * may use a faster but less crash-safe code path (direct write without
   * temp+rename) since data loss on crash is recoverable.
   *
   * Callers that flush WAL state into the base file (maybeCompactWal,
   * loadOverlayDir's WAL merge, recursive removal cleanup) must pass
   * isMaterialized=true explicitly. WAL-tracked directories are by
   * definition materialized — letting a non-crash-safe O_TRUNC rewrite
   * race with a crash on those directories would leave a truncated base
   * file plus a dropped WAL = lost user data.
   */
  void saveOverlayDir(
      InodeNumber inodeNumber,
      const DirContents& dir,
      bool isMaterialized = true);

  /*
   * Load content of the directory from overlay. If the directory does not
   * exist, this function will return an empty `DirContents`.
   */
  DirContents loadOverlayDir(InodeNumber inodeNumber);

  void removeOverlayFile(InodeNumber inodeNumber);

  void removeOverlayDir(InodeNumber inodeNumber);

  /**
   * Remove the overlay data for the given tree inode and recursively remove
   * everything beneath it too.
   *
   * Must only be called on trees.
   */
  void recursivelyRemoveOverlayDir(InodeNumber inodeNumber);

  /**
   * Like recursivelyRemoveOverlayDir, but performs the work on the background
   * GC thread instead of blocking the caller.
   */
  void recursivelyRemoveOverlayDirBackground(InodeNumber inodeNumber);

  /**
   * Returns a future that completes once all previously-issued async
   * operations, namely recursivelyRemoveOverlayDir, finish.
   */
  folly::Future<folly::Unit> flushPendingAsync();

  bool hasOverlayDir(InodeNumber inodeNumber);

#ifndef _WIN32
  bool hasOverlayFile(InodeNumber inodeNumber);

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
   * Some inode catalogs (SqliteInodeCatalog) requires periodic task to maintain
   * their underlying storage. This method is called periodically by an
   * EdenServer task in the configured interval.
   */
  void maintenance();

  /*
   * Returns a raw pointer to the inode catalog. This method should only be
   * used for testing.
   */
  InodeCatalog* getRawInodeCatalog() {
    return inodeCatalog_.get();
  }

  FileContentStore* getRawFileContentStore() {
    return fileContentStore_.get();
  }

  InodeCatalogType getInodeCatalogType() const {
    return inodeCatalogType_;
  }

  /**
   * Build a Thrift `overlay::OverlayDir` from in-memory `DirContents`.
   */
  [[deprecated(
      "Prefer direct serialization via InodeCatalog::saveOverlayEntries")]]
  overlay::OverlayDir serializeOverlayDir(
      InodeNumber inodeNumber,
      const DirContents& dir);

  const AbsolutePath& getLocalDir() const {
    return localDir_;
  }

  void setFsckSemaphore(folly::LifoSem* sem) {
    fsckSemaphore_ = sem;
  }

  /**
   * Optional callback invoked inside the fsck critical section
   * (after semaphore acquire, before fsck work). Used by tests
   * to observe concurrency behavior.
   */
  void setFsckCallback(folly::Function<void()> cb) {
    fsckCallback_ = std::move(cb);
  }

  /**
   * Optional callback invoked just before the fsck semaphore wait.
   * Used by tests to know when a thread has reached the semaphore.
   */
  void setPreFsckSemaphoreCallback(folly::Function<void()> cb) {
    preFsckSemaphoreCallback_ = std::move(cb);
  }

 private:
  explicit Overlay(
      AbsolutePathPiece localDir,
      CaseSensitivity caseSensitive,
      InodeCatalogType inodeCatalogType,
      InodeCatalogOptions inodeCatalogOptions,
      std::shared_ptr<EdenFsEventsLogger> logger,
      EdenStatsPtr stats,
      const EdenConfig& config);

  /**
   * Returns true if the overlay should use WAL (Write-Ahead Log) for
   * deferred directory writes. Computed once at construction as
   * `overlayUseWal` config flag AND `InodeCatalog::supportsWal()`.
   */
  bool useWal() const {
    return useWal_;
  }

  /**
   * Whether this overlay's backing catalog can have WAL files on disk.
   * Used for replay — we always replay WAL files if they exist,
   * regardless of whether WAL is currently enabled via config. This
   * ensures safe rollback when disabling the WAL.
   */
  bool canHaveWalFiles() const {
    return inodeCatalog_->supportsWal();
  }

  friend class WalGuardTest_useWalRequiresFlagAndLegacyCatalog_Test;
  friend class WalGuardTest_canHaveWalFilesIgnoresFlag_Test;
  friend class WalCompactionTest_belowThresholdRetainsWal_Test;
  friend class WalCompactionTest_exceedsThresholdTriggersFullSave_Test;
  friend class WalCompactionTest_largeBaseSizeScalesThreshold_Test;
  friend class WalCompactionTest_nonWalCatalogDoesNotTrackCompaction_Test;
  friend class WalCompactionTest_thresholdIsCappedForLargeDirectories_Test;
  friend class WalCompactionTest_recompactsAfterReset_Test;

  /**
   * Increment the per-parent WAL entry counter and, if it crosses the
   * inline-compaction threshold, rewrite the parent directory file from
   * `content` and clear the WAL.
   *
   * Threshold is `min(walCompactionMultiplier_ * max(content.size() - count,
   * 10), walCompactionCap_)`. For tiny directories the floor of 10 keeps a
   * small constant overhead; for larger directories the threshold scales with
   * the base size so the amortized rewrite cost stays O(1) per WAL append. The
   * cap bounds the worst-case latency of the compaction itself: without it, a
   * million-entry directory could let the WAL grow to ~3M entries before the
   * inline rewrite stalls every other op on the directory.
   *
   * Compaction is intentionally inline (rather than on a background
   * thread) — the existing `saveOverlayDir` path also runs synchronously
   * under the parent contents lock, so a WAL write's worst case matches
   * pre-WAL behavior while its expected case is O(1). A background
   * compactor would need to coordinate with mutators on the same lock
   * and offers no asymptotic improvement.
   *
   * Callers must hold the parent TreeInode's contents lock so that
   * `content` is consistent with the WAL file on disk.
   */
  void maybeCompactWal(InodeNumber parent, const DirContents& content);

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

    /**
     * Request to recursively remove an overlay directory tree in the
     * background. The GC thread will load, remove, and recurse into children.
     */
    explicit GCRequest(InodeNumber ino) : requestType{ino} {}

    std::variant<
        MaintenanceRequest,
        overlay::OverlayDir,
        FlushRequest,
        InodeNumber>
        requestType;
  };

  struct GCQueue {
    bool stop = false;
    std::vector<GCRequest> queue;
  };

  void initOverlay(
      std::shared_ptr<ReloadableConfig> config,
      std::optional<AbsolutePath> mountPath,
      const OverlayChecker::ProgressCallback& progressCallback,
      InodeCatalog::LookupCallback& lookupCallback);
  void gcThread() noexcept;
  void handleGCRequest(GCRequest& request);

  // Serialize EdenFS overlay data structure into Thrift data structure
  overlay::OverlayEntry serializeOverlayEntry(const DirEntry& entry);

  using OverlayEntryVisitor = folly::FunctionRef<
      void(const std::string& name, const overlay::OverlayEntry& entry)>;
  using OverlayEntrySource =
      folly::FunctionRef<void(OverlayEntryVisitor visitor)>;

  /**
   * Iterate DirContents, validate each entry, serialize to OverlayEntry,
   * and pass to the visitor. This is the save-side counterpart to
   * buildDirEntries.
   */
  void visitDirEntries(
      InodeNumber inodeNumber,
      const DirContents& dir,
      OverlayEntryVisitor visitor);

  /**
   * Process overlay entries from the given source, handling inode allocation
   * and AppleDouble filtering. Appends processed entries to the output vector.
   * Returns true if the overlay should be rewritten.
   */
  bool buildDirEntries(
      OverlayEntrySource source,
      folly::fbvector<std::pair<PathComponent, DirEntry>>& entries);

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

  std::unique_ptr<FileContentStore> fileContentStore_;
  /**
   * Cached non-owning pointer to fileContentStore_ when it is an
   * FsFileContentStore (nullptr otherwise). Lets WAL fast paths avoid a
   * per-call dynamic_cast on the hot directory-mutation path. Set in
   * initOverlay() and never reassigned. Must outlive every WAL caller —
   * fileContentStore_ owns the storage, so this pointer is valid as long
   * as the Overlay is alive.
   */
  FsFileContentStore* fsCore_{nullptr};
  std::unique_ptr<InodeCatalog> inodeCatalog_;
  InodeCatalogType inodeCatalogType_;
  InodeCatalogOptions inodeCatalogOptions_;

  /**
   * Indicates if the backing overlay supports semantic operations, see
   * `InodeCatalog::supportsSemanticOperations` for more information.
   */
  bool supportsSemanticOperations_;

  bool filterAppleDouble_;

  const AbsolutePath localDir_;

#ifndef _WIN32
  /**
   * Disk-backed mapping from inode number to InodeMetadata.
   * Defined below inodeCatalog_ because it acquires its own file lock, which
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
   * acts a boolean closed: True if the Overlay has been closed with
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

  std::shared_ptr<EdenFsEventsLogger> edenFsEventsLogger_;
  EdenStatsPtr stats_;

  // Borrowed from EdenServer. Valid for Overlay's lifetime because
  // EdenServer::unmountAll() completes before semaphore destruction.
  folly::LifoSem* fsckSemaphore_{nullptr};

  folly::Function<void()> fsckCallback_;
  folly::Function<void()> preFsckSemaphoreCallback_;

  friend class IORequest;

  bool useDirectFileWrites_;

  bool useWal_{false};
  size_t walCompactionMultiplier_{3};
  size_t walCompactionCap_{100'000};

  /**
   * Drop any WAL state for `parent` after a full directory rewrite or
   * removal. Idempotent (safe to call when no WAL exists). No-op for
   * catalog types that never have WAL files on disk.
   */
  void clearWalAfterFullWrite(InodeNumber parent);

  /**
   * Replay any pending WAL entries for `parent` into `dir` so callers
   * that consume the loaded directory (e.g., recursive removal) see the
   * merged state.
   *
   * Steady-state cost is one fstatat (via hasWal) when no WAL exists;
   * the dynamic_cast and replay are skipped entirely. When a WAL does
   * exist, the cost is bounded by the inline-compaction threshold
   * (3 * max(dirSize, 10)) — the deferred work the WAL was hiding.
   *
   * Caller is responsible for cleaning up the on-disk WAL file
   * afterwards (e.g., via clearWalAfterFullWrite); this helper only
   * mutates `dir`.
   */
  void mergeWalIntoOverlayDir(InodeNumber parent, overlay::OverlayDir& dir);

  /**
   * Sharded in-memory count of WAL entries per directory inode, used to
   * trigger inline compaction. Sharding by inode-number prevents the
   * shared mutex from becoming a global bottleneck during parallel
   * checkout (where every TreeInode mutation contends).
   *
   * Intentionally a process-global structure rather than a member on
   * `TreeInode` / `DirContents`: the WAL append path runs under the
   * parent's contents lock, so storing the counter on the inode would
   * either piggyback on that lock (forcing the counter increment into
   * the same critical section) or require an additional per-inode
   * mutex just for this counter. The sharded global keeps the counter
   * update to a leaf-level lock acquisition that can be released
   * before saveOverlayDir runs. Counters are inserted on append and erased on
   * full rewrite or removal via clearWalAfterFullWrite.
   *
   * Does not need to survive restart — on load, WAL files are replayed
   * and compacted immediately.
   */
  static constexpr size_t kWalEntryCountsShards = 64;
  std::array<
      folly::Synchronized<folly::F14FastMap<InodeNumber, size_t>>,
      kWalEntryCountsShards>
      walEntryCountsShards_;

  /**
   * Pick the shard for a given inode number.
   */
  folly::Synchronized<folly::F14FastMap<InodeNumber, size_t>>&
  walEntryCountsShard(InodeNumber inode) {
    return walEntryCountsShards_[inode.get() % kWalEntryCountsShards];
  }
};

constexpr InodeCatalogType kDefaultInodeCatalogType =
    folly::kIsWindows ? InodeCatalogType::Sqlite : InodeCatalogType::Legacy;
constexpr InodeCatalogOptions kDefaultInodeCatalogOptions =
    INODE_CATALOG_DEFAULT;

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
          "cannot access overlay after it is closed");
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
