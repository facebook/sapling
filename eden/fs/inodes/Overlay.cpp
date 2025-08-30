/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/Overlay.h"

#include <boost/filesystem.hpp>
#include <algorithm>

#include <folly/Exception.h>
#include <folly/File.h>
#include <folly/FileUtil.h>
#include <folly/Range.h>
#include <folly/io/IOBuf.h>
#include <folly/logging/xlog.h>
#include <folly/stop_watch.h>
#include <thrift/lib/cpp2/protocol/Serializer.h>

#include "eden/common/utils/Bug.h"
#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/inodes/DirEntry.h"
#include "eden/fs/inodes/FileContentStore.h"
#include "eden/fs/inodes/InodeBase.h"
#include "eden/fs/inodes/InodeTable.h"
#include "eden/fs/inodes/OverlayFile.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/inodes/memcatalog/MemInodeCatalog.h"
#include "eden/fs/inodes/sqlitecatalog/BufferedSqliteInodeCatalog.h"
#include "eden/fs/inodes/sqlitecatalog/SqliteInodeCatalog.h"
#include "eden/fs/sqlite/SqliteDatabase.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/LogEvent.h"

#ifndef _WIN32
#include "eden/fs/inodes/lmdbcatalog/BufferedLMDBInodeCatalog.h" // @manual
#include "eden/fs/inodes/lmdbcatalog/LMDBFileContentStore.h" // @manual
#include "eden/fs/inodes/lmdbcatalog/LMDBInodeCatalog.h" // @manual
#endif

namespace facebook::eden {

namespace {
constexpr uint64_t ioCountMask = 0x7FFFFFFFFFFFFFFFull;
constexpr uint64_t ioClosedMask = 1ull << 63;

std::unique_ptr<InodeCatalog> makeInodeCatalog(
    AbsolutePathPiece localDir,
    InodeCatalogType inodeCatalogType,
    InodeCatalogOptions inodeCatalogOptions,
    const EdenConfig& config,
    FileContentStore* fileContentStore,
    const std::shared_ptr<StructuredLogger>& logger) {
  if (inodeCatalogType == InodeCatalogType::Sqlite) {
    // Controlled via EdenConfig::unsafeInMemoryOverlay
    if (inodeCatalogOptions.containsAllOf(INODE_CATALOG_UNSAFE_IN_MEMORY)) {
      // Controlled via EdenConfig::overlayBuffered
      if (inodeCatalogOptions.containsAllOf(INODE_CATALOG_BUFFERED)) {
        XLOG(
            WARN,
            "In-memory Sqlite buffered overlay requested. This will cause data loss.");
        return std::make_unique<BufferedSqliteInodeCatalog>(
            std::make_unique<SqliteDatabase>(SqliteDatabase::inMemory), config);
      } else {
        XLOG(
            WARN,
            "In-memory Sqlite overlay requested. This will cause data loss.");
        return std::make_unique<SqliteInodeCatalog>(
            std::make_unique<SqliteDatabase>(SqliteDatabase::inMemory));
      }
    }
    // Controlled via EdenConfig::overlaySynchronousMode
    if (inodeCatalogOptions.containsAllOf(INODE_CATALOG_SYNCHRONOUS_OFF)) {
      // Controlled via EdenConfig::overlayBuffered
      if (inodeCatalogOptions.containsAllOf(INODE_CATALOG_BUFFERED)) {
        XLOG(
            DBG2,
            "Buffered Sqlite overlay being used with synchronous-mode = off");
        return std::make_unique<BufferedSqliteInodeCatalog>(
            localDir, logger, config, SqliteTreeStore::SynchronousMode::Off);
      } else {
        XLOG(DBG2, "Sqlite overlay being used with synchronous-mode = off");
        return std::make_unique<SqliteInodeCatalog>(
            localDir, logger, SqliteTreeStore::SynchronousMode::Off);
      }
    }
    // Controlled via EdenConfig::overlayBuffered
    if (inodeCatalogOptions.containsAllOf(INODE_CATALOG_BUFFERED)) {
      XLOG(DBG4, "Buffered Sqlite overlay being used");
      return std::make_unique<BufferedSqliteInodeCatalog>(
          localDir, logger, config);
    }
    XLOG(DBG4, "Sqlite overlay being used.");
    return std::make_unique<SqliteInodeCatalog>(localDir, logger);
  } else if (inodeCatalogType == InodeCatalogType::InMemory) {
    XLOG(DBG4, "In-memory overlay being used.");
    return std::make_unique<MemInodeCatalog>();
  }
#ifdef _WIN32
  (void)fileContentStore;
  if (inodeCatalogType == InodeCatalogType::Legacy) {
    throw std::runtime_error(
        "Legacy overlay type is not supported. Please reclone.");
  } else if (inodeCatalogType == InodeCatalogType::LMDB) {
    throw std::runtime_error(
        "LMDB overlay type is not supported. Please reclone.");
  }
  XLOG(DBG4, "Sqlite overlay being used.");
  return std::make_unique<SqliteInodeCatalog>(localDir, logger);
#else
  if (inodeCatalogType == InodeCatalogType::LMDB) {
    if (inodeCatalogOptions.containsAllOf(INODE_CATALOG_BUFFERED)) {
      XLOG(DBG4, "Buffered LMDB overlay being used");
      return std::make_unique<BufferedLMDBInodeCatalog>(
          static_cast<LMDBFileContentStore*>(fileContentStore), config);
    }
    XLOG(DBG4, "LMDB overlay being used");
    return std::make_unique<LMDBInodeCatalog>(
        static_cast<LMDBFileContentStore*>(fileContentStore));
  }
  if (inodeCatalogType == InodeCatalogType::LegacyDev) {
    XLOG(DBG4, "LegacyDev overlay being used.");
    return std::make_unique<FsInodeCatalogDev>(
        static_cast<FsFileContentStoreDev*>(fileContentStore));
  }
  XLOG(DBG4, "Legacy overlay being used.");
  return std::make_unique<FsInodeCatalog>(
      static_cast<FsFileContentStore*>(fileContentStore));
#endif
}

std::unique_ptr<FileContentStore> makeFileContentStore(
    AbsolutePathPiece localDir,
    const std::shared_ptr<StructuredLogger>& logger,
    InodeCatalogType inodeCatalogType) {
#ifdef _WIN32
  (void)localDir;
  (void)logger;
  return nullptr;
#else
  if (inodeCatalogType == InodeCatalogType::Legacy) {
    return std::make_unique<FsFileContentStore>(localDir);
  } else if (inodeCatalogType == InodeCatalogType::LegacyDev) {
    return std::make_unique<FsFileContentStoreDev>(localDir);
  } else {
    return std::make_unique<LMDBFileContentStore>(localDir, logger);
  }
#endif
}
} // namespace

using folly::Unit;
using std::optional;

std::shared_ptr<Overlay> Overlay::create(
    AbsolutePathPiece localDir,
    CaseSensitivity caseSensitive,
    InodeCatalogType inodeCatalogType,
    InodeCatalogOptions inodeCatalogOptions,
    std::shared_ptr<StructuredLogger> logger,
    EdenStatsPtr stats,
    bool windowsSymlinksEnabled,
    const EdenConfig& config) {
  // This allows us to access the private constructor.
  struct MakeSharedEnabler : public Overlay {
    explicit MakeSharedEnabler(
        AbsolutePathPiece localDir,
        CaseSensitivity caseSensitive,
        InodeCatalogType inodeCatalogType,
        InodeCatalogOptions inodeCatalogOptions,
        std::shared_ptr<StructuredLogger> logger,
        EdenStatsPtr stats,
        bool windowsSymlinksEnabled,
        const EdenConfig& config)
        : Overlay(
              localDir,
              caseSensitive,
              inodeCatalogType,
              inodeCatalogOptions,
              logger,
              std::move(stats),
              windowsSymlinksEnabled,
              config) {}
  };
  return std::make_shared<MakeSharedEnabler>(
      localDir,
      caseSensitive,
      inodeCatalogType,
      inodeCatalogOptions,
      logger,
      std::move(stats),
      windowsSymlinksEnabled,
      config);
}

Overlay::Overlay(
    AbsolutePathPiece localDir,
    CaseSensitivity caseSensitive,
    InodeCatalogType inodeCatalogType,
    InodeCatalogOptions inodeCatalogOptions,
    std::shared_ptr<StructuredLogger> logger,
    EdenStatsPtr stats,
    bool windowsSymlinksEnabled,
    const EdenConfig& config)
    : fileContentStore_{makeFileContentStore(
          localDir,
          logger,
          inodeCatalogType)},
      inodeCatalog_{makeInodeCatalog(
          localDir,
          inodeCatalogType,
          inodeCatalogOptions,
          config,
          fileContentStore_ ? fileContentStore_.get() : nullptr,
          logger)},
      inodeCatalogType_{inodeCatalogType},
      inodeCatalogOptions_(inodeCatalogOptions),
      supportsSemanticOperations_{inodeCatalog_->supportsSemanticOperations()},
      filterAppleDouble_{
          folly::kIsApple && !config.allowAppleDouble.getValue()},
      localDir_{localDir},
      caseSensitive_{caseSensitive},
      structuredLogger_{logger},
      stats_{std::move(stats)},
      windowsSymlinksEnabled_(windowsSymlinksEnabled) {}

Overlay::~Overlay() {
  close();
}

void Overlay::close() {
  XCHECK_NE(std::this_thread::get_id(), gcThread_.get_id());

  gcQueue_.lock()->stop = true;
  gcCondVar_.notify_one();
  if (gcThread_.joinable()) {
    gcThread_.join();
  }

  // Make sure everything is shut down in reverse of construction order.
  // Cleanup is not necessary if tree overlay was not initialized and either
  // there is no file content store or the it was not initialized
  if (!inodeCatalog_->initialized() &&
      (!fileContentStore_ ||
       (fileContentStore_ && !fileContentStore_->initialized()))) {
    return;
  }

  // Since we are closing the overlay, no other threads can still be using
  // it. They must have used some external synchronization mechanism to
  // ensure this, so it is okay for us to still use relaxed access to
  // nextInodeNumber_.
  std::optional<InodeNumber> optNextInodeNumber;
  auto nextInodeNumber = nextInodeNumber_.load(std::memory_order_relaxed);
  if (nextInodeNumber) {
    optNextInodeNumber = InodeNumber{nextInodeNumber};
  }

  closeAndWaitForOutstandingIO();
#ifndef _WIN32
  inodeMetadataTable_.reset();
#endif // !_WIN32

  inodeCatalog_->close(optNextInodeNumber);
  if (fileContentStore_ && inodeCatalogType_ != InodeCatalogType::Legacy &&
      inodeCatalogType_ != InodeCatalogType::LegacyDev) {
    fileContentStore_->close();
  }
}

bool Overlay::isClosed() {
  return outstandingIORequests_.load(std::memory_order_acquire) & ioClosedMask;
}

#ifndef _WIN32
struct statfs Overlay::statFs() {
  IORequest req{this};
  XCHECK(fileContentStore_);
  return fileContentStore_->statFs();
}
#endif // !_WIN32

folly::SemiFuture<Unit> Overlay::initialize(
    std::shared_ptr<const EdenConfig> config,
    std::optional<AbsolutePath> mountPath,
    OverlayChecker::ProgressCallback&& progressCallback,
    InodeCatalog::LookupCallback&& lookupCallback) {
  // The initOverlay() call is potentially slow, so we want to avoid
  // performing it in the current thread and blocking returning to our caller.
  //
  // We already spawn a separate thread for garbage collection.  It's convenient
  // to simply use this existing thread to perform the initialization logic
  // before waiting for GC work to do.
  auto [initPromise, initFuture] = folly::makePromiseContract<Unit>();

  gcThread_ = std::thread([this,
                           config = std::move(config),
                           mountPath = std::move(mountPath),
                           progressCallback = std::move(progressCallback),
                           lookupCallback = lookupCallback,
                           promise = std::move(initPromise)]() mutable {
    try {
      initOverlay(
          std::move(config),
          std::move(mountPath),
          progressCallback,
          lookupCallback);
    } catch (...) {
      auto ew = folly::exception_wrapper{std::current_exception()};
      XLOGF(
          ERR,
          "overlay initialization failed for {}: {}",
          localDir_,
          folly::exceptionStr(ew));
      promise.setException(std::move(ew));
      return;
    }
    promise.setValue();

    gcThread();
  });
  return std::move(initFuture);
}

void Overlay::initOverlay(
    std::shared_ptr<const EdenConfig> config,
    std::optional<AbsolutePath> mountPath,
    [[maybe_unused]] const OverlayChecker::ProgressCallback& progressCallback,
    [[maybe_unused]] InodeCatalog::LookupCallback& lookupCallback) {
  IORequest req{this};
  auto optNextInodeNumber =
      inodeCatalog_->initOverlay(/*createIfNonExisting=*/true);
  if (fileContentStore_ && inodeCatalogType_ == InodeCatalogType::Sqlite) {
    // Initialize the file content store after the inode catalog has been.
    // The fileContentStore will only exist on non-Windows platforms.
    //
    // We only need to do this for Sqlite overlays because they use a Legacy
    // FileContentStore on non-Windows platforms. Other InodeCatalogTypes use
    // their corresponding FileContentStore, meaning calling `initialize` here
    // would double-initialize the FileContentStore the objects.
    //
    // If we had a SQLiteFileContentStore, this code block would be unnecessary.
    fileContentStore_->initialize(/*createIfNonExisting=*/true);
  }
  if (!optNextInodeNumber.has_value()) {
#ifndef _WIN32
    // FSCK is not currently supported for LMDB overlays. If we cannot load the
    // next inode number, then we cannot continue. LMDB should always be able to
    // load the inode number, if this case is hit, then the assumption about
    // LMDB being resilient is incorrect (unless the user manually corrupted
    // their overlay directory).
    if (inodeCatalogType_ != InodeCatalogType::Legacy) {
      throw std::runtime_error(
          "Corrupted LMDB overlay " + localDir_.asString() +
          ": could not load next inode number");
    }
    // If the next-inode-number data is missing it means that this overlay was
    // not shut down cleanly the last time it was used.  If this was caused by a
    // hard system reboot this can sometimes cause corruption and/or missing
    // data in some of the on-disk state.
    //
    // Use OverlayChecker to scan the overlay for any issues, and also compute
    // correct next inode number as it does so.
    XLOGF(
        WARN,
        "Overlay {} was not shut down cleanly.  Performing fsck scan.",
        localDir_);

    // TODO(zeyi): `OverlayCheck` should be associated with the specific
    // Overlay implementation.
    //
    // Note: lookupCallback is a reference but is stored on OverlayChecker.
    // Therefore OverlayChecker must not exist longer than this initOverlay
    // call.
    OverlayChecker checker(
        inodeCatalog_.get(),
        static_cast<FsFileContentStore*>(fileContentStore_.get()),
        std::nullopt,
        lookupCallback,
        config->fsckNumErrorDiscoveryThreads.getValue());
    folly::stop_watch<> fsckRuntime;
    checker.scanForErrors(progressCallback);
    auto result = checker.repairErrors();
    auto fsckRuntimeInSeconds =
        std::chrono::duration<double>{fsckRuntime.elapsed()}.count();
    if (result) {
      // If totalErrors - fixedErrors is nonzero, then we failed to
      // fix all of the problems.
      auto success = !(result->totalErrors - result->fixedErrors);
      structuredLogger_->logEvent(
          Fsck{fsckRuntimeInSeconds, success, true /*attempted_repair*/});
    } else {
      structuredLogger_->logEvent(Fsck{
          fsckRuntimeInSeconds, true /*success*/, false /*attempted_repair*/});
    }

    optNextInodeNumber = checker.getNextInodeNumber();
#else
    // SqliteInodeCatalog will always return the value of next Inode number, if
    // we end up here - it's a bug.
    EDEN_BUG() << "Tree Overlay is null value for NextInodeNumber";
#endif
  } else {
    hadCleanStartup_ = true;
  }

  // On Windows, we need to scan the state of the repository every time at
  // start up to find any potential changes happened when EdenFS is not
  // running.
  //
  // mountPath will be empty during benchmarking so we must check the value
  // here to skip scanning in that case.
  if (folly::kIsWindows && mountPath.has_value()) {
    folly::stop_watch<> fsckRuntime;
    optNextInodeNumber = inodeCatalog_->scanLocalChanges(
        std::move(config), *mountPath, windowsSymlinksEnabled_, lookupCallback);
    auto fsckRuntimeInSeconds =
        std::chrono::duration<double>{fsckRuntime.elapsed()}.count();
    structuredLogger_->logEvent(Fsck{
        fsckRuntimeInSeconds, true /*success*/, false /*attempted_repair*/});
  }

  nextInodeNumber_.store(optNextInodeNumber->get(), std::memory_order_relaxed);

#ifndef _WIN32
  // Open after infoFile_'s lock is acquired because the InodeTable acquires
  // its own lock, which should be released prior to infoFile_.
  inodeMetadataTable_ = InodeMetadataTable::open(
      (localDir_ + PathComponentPiece{FsFileContentStore::kMetadataFile})
          .c_str(),
      stats_.copy());
#endif // !_WIN32
}

InodeNumber Overlay::allocateInodeNumber() {
  // InodeNumber should generally be 64-bits wide, in which case it isn't even
  // worth bothering to handle the case where nextInodeNumber_ wraps.  We don't
  // need to bother checking for conflicts with existing inode numbers since
  // this can only happen if we wrap around.  We don't currently support
  // platforms with 32-bit inode numbers.
  static_assert(
      sizeof(nextInodeNumber_) == sizeof(InodeNumber),
      "expected nextInodeNumber_ and InodeNumber to have the same size");
  static_assert(
      sizeof(InodeNumber) >= 8, "expected InodeNumber to be at least 64 bits");

  // This could be a relaxed atomic operation.  It doesn't matter on x86 but
  // might on ARM.
  auto previous = nextInodeNumber_++;
  XDCHECK_NE(0u, previous) << "allocateInodeNumber called before initialize";
  return InodeNumber{previous};
}

DirContents Overlay::loadOverlayDir(InodeNumber inodeNumber) {
  DurationScope<EdenStats> statScope{stats_, &OverlayStats::loadOverlayDir};
  DirContents result(caseSensitive_);
  IORequest req{this};
  auto dirData = inodeCatalog_->loadOverlayDir(inodeNumber);
  if (!dirData.has_value()) {
    stats_->increment(&OverlayStats::loadOverlayDirFailure);
    return result;
  }
  const auto& dir = dirData.value();

  bool shouldRewriteOverlay = false;

  for (auto& iter : *dir.entries()) {
    const auto& name = iter.first;
    const auto& value = iter.second;

    // If AppleDouble files (._) need to be filtered, omit them from the
    // returned DirContents and rewrite the overlay directory to remove them
    // from the Overlay entirely.
    if (filterAppleDouble_ && string_view{name}.starts_with("._")) {
      shouldRewriteOverlay = true;
      continue;
    }

    InodeNumber ino;
    if (*value.inodeNumber()) {
      ino = InodeNumber::fromThrift(*value.inodeNumber());
    } else {
      ino = allocateInodeNumber();
      shouldRewriteOverlay = true;
    }

    if (value.hash() && !value.hash()->empty()) {
      auto hash = ObjectId{folly::ByteRange{folly::StringPiece{*value.hash()}}};
      result.emplace(PathComponentPiece{name}, *value.mode(), ino, hash);
    } else {
      // The inode is materialized
      result.emplace(PathComponentPiece{name}, *value.mode(), ino);
    }
  }

  if (shouldRewriteOverlay) {
    saveOverlayDir(inodeNumber, result);
  }
  stats_->increment(&OverlayStats::loadOverlayDirSuccessful);
  return result;
}

overlay::OverlayEntry Overlay::serializeOverlayEntry(const DirEntry& ent) {
  overlay::OverlayEntry entry;

  // TODO: Eventually, we should only serialize the child entry's dtype into
  // the Overlay. But, as of now, it's possible to create an inode under a
  // tree, serialize that tree into the overlay, then restart Eden. Since
  // writing mode bits into the InodeMetadataTable only occurs when the inode
  // is loaded, the initial mode bits must persist until the first load.
  entry.mode() = ent.getInitialMode();
  entry.inodeNumber() = ent.getInodeNumber().get();
  if (!ent.isMaterialized()) {
    entry.hash() = ent.getObjectId().asString();
  }

  return entry;
}

overlay::OverlayDir Overlay::serializeOverlayDir(
    InodeNumber inodeNumber,
    const DirContents& dir) {
  IORequest req{this};
  auto nextInodeNumber = nextInodeNumber_.load(std::memory_order_relaxed);
  XCHECK_LT(inodeNumber.get(), nextInodeNumber)
      << "serializeOverlayDir called with unallocated inode number";

  // TODO: T20282158 clean up access of child inode information.
  //
  // Translate the data to the thrift equivalents
  overlay::OverlayDir odir;

  for (auto& entIter : dir) {
    const auto& entName = entIter.first;
    const auto& ent = entIter.second;

    XCHECK_NE(entName, "") << fmt::format(
        "serializeOverlayDir called with entry with an empty path for directory with inodeNumber={}",
        inodeNumber);
    XCHECK_LT(ent.getInodeNumber().get(), nextInodeNumber)
        << "serializeOverlayDir called with entry using unallocated inode number";

    odir.entries()->emplace(
        std::make_pair(entName.asString(), serializeOverlayEntry(ent)));
  }

  return odir;
}

void Overlay::saveOverlayDir(InodeNumber inodeNumber, const DirContents& dir) {
  DurationScope<EdenStats> statScope{stats_, &OverlayStats::saveOverlayDir};
  try {
    inodeCatalog_->saveOverlayDir(
        inodeNumber, serializeOverlayDir(inodeNumber, dir));
    stats_->increment(&OverlayStats::saveOverlayDirSuccessful);
  } catch (const std::exception& e) {
    XLOGF(ERR, "Failed to save overlay dir {} {}", inodeNumber, e.what());
    stats_->increment(&OverlayStats::saveOverlayDirFailure);
    throw;
  }
}

void Overlay::freeInodeFromMetadataTable(InodeNumber ino) {
#ifndef _WIN32
  // TODO: batch request during GC
  getInodeMetadataTable()->freeInode(ino);
#else
  (void)ino;
#endif
}

void Overlay::removeOverlayFile(InodeNumber inodeNumber) {
  DurationScope<EdenStats> statScope{stats_, &OverlayStats::removeOverlayFile};
  try {
#ifndef _WIN32
    IORequest req{this};

    freeInodeFromMetadataTable(inodeNumber);
    fileContentStore_->removeOverlayFile(inodeNumber);
    stats_->increment(&OverlayStats::removeOverlayFileSuccessful);
#else
    (void)inodeNumber;
#endif
  } catch (const std::exception& e) {
    XLOGF(ERR, "Failed to remove overlay file {} {}", inodeNumber, e.what());
    stats_->increment(&OverlayStats::removeOverlayFileFailure);
    throw;
  }
}

void Overlay::removeOverlayDir(InodeNumber inodeNumber) {
  DurationScope<EdenStats> statScope{stats_, &OverlayStats::removeOverlayDir};
  try {
    IORequest req{this};

    freeInodeFromMetadataTable(inodeNumber);
    inodeCatalog_->removeOverlayDir(inodeNumber);
    stats_->increment(&OverlayStats::removeOverlayDirSuccessful);
  } catch (const std::exception& e) {
    XLOGF(ERR, "Failed to remove overlay dir {} {}", inodeNumber, e.what());
    stats_->increment(&OverlayStats::removeOverlayDirFailure);
    throw;
  }
}

void Overlay::recursivelyRemoveOverlayDir(InodeNumber inodeNumber) {
  DurationScope<EdenStats> statScope{
      stats_, &OverlayStats::recursivelyRemoveOverlayDir};
  try {
    IORequest req{this};
    freeInodeFromMetadataTable(inodeNumber);

    // This inode's data must be removed from the overlay before
    // recursivelyRemoveOverlayDir returns to avoid a race condition if
    // recursivelyRemoveOverlayDir(I) is called immediately prior to
    // saveOverlayDir(I).  There's also no risk of violating our durability
    // guarantees if the process dies after this call but before the thread
    // could remove this data.
    auto dirData = inodeCatalog_->loadAndRemoveOverlayDir(inodeNumber);
    if (dirData) {
      gcQueue_.lock()->queue.emplace_back(std::move(*dirData));
      gcCondVar_.notify_one();
      stats_->increment(&OverlayStats::recursivelyRemoveOverlayDirSuccessful);
    }
  } catch (const std::exception& e) {
    XLOGF(
        ERR,
        "Failed to recursively remove overlay dir {} {}",
        inodeNumber,
        e.what());
    stats_->increment(&OverlayStats::recursivelyRemoveOverlayDirFailure);
    throw;
  }
}

#ifndef _WIN32
folly::Future<folly::Unit> Overlay::flushPendingAsync() {
  folly::Promise<folly::Unit> promise;
  auto future = promise.getFuture();
  gcQueue_.lock()->queue.emplace_back(std::move(promise));
  gcCondVar_.notify_one();
  return future;
}
#endif // !_WIN32

bool Overlay::hasOverlayDir(InodeNumber inodeNumber) {
  DurationScope<EdenStats> statScope{stats_, &OverlayStats::hasOverlayDir};
  try {
    IORequest req{this};

    bool has_overlay_dir = inodeCatalog_->hasOverlayDir(inodeNumber);
    stats_->increment(&OverlayStats::hasOverlayDirSuccessful);
    return has_overlay_dir;
  } catch (const std::exception& e) {
    XLOGF(
        ERR,
        "Failed to check if overlay dir exists {} {}",
        inodeNumber,
        e.what());
    stats_->increment(&OverlayStats::hasOverlayDirFailure);
    throw;
  }
}

#ifndef _WIN32

bool Overlay::hasOverlayFile(InodeNumber inodeNumber) {
  DurationScope<EdenStats> statScope{stats_, &OverlayStats::hasOverlayFile};
  try {
    IORequest req{this};
    XCHECK(fileContentStore_);
    bool has_overlay_file = fileContentStore_->hasOverlayFile(inodeNumber);
    stats_->increment(&OverlayStats::hasOverlayFileSuccessful);
    return has_overlay_file;
  } catch (const std::exception& e) {
    XLOGF(
        ERR,
        "Failed to check if overlay file exists {} {}",
        inodeNumber,
        e.what());
    stats_->increment(&OverlayStats::hasOverlayFileFailure);
    throw;
  }
}

// Helper function to open,validate,
// get file pointer of an overlay file
OverlayFile Overlay::openFile(
    InodeNumber inodeNumber,
    folly::StringPiece headerId) {
  DurationScope<EdenStats> statScope{stats_, &OverlayStats::openOverlayFile};
  IORequest req{this};
  try {
    XCHECK(fileContentStore_);
    auto file = OverlayFile(
        fileContentStore_->openFile(inodeNumber, headerId), weak_from_this());
    stats_->increment(&OverlayStats::openOverlayFileSuccessful);
    return file;
  } catch (const std::exception& e) {
    XLOGF(ERR, "Failed to open file {} {} {}", inodeNumber, headerId, e.what());
    stats_->increment(&OverlayStats::openOverlayFileFailure);
    throw;
  }
}

OverlayFile Overlay::openFileNoVerify(InodeNumber inodeNumber) {
  DurationScope<EdenStats> statScope{stats_, &OverlayStats::openOverlayFile};
  IORequest req{this};
  try {
    XCHECK(fileContentStore_);
    auto file = OverlayFile(
        fileContentStore_->openFileNoVerify(inodeNumber), weak_from_this());
    stats_->increment(&OverlayStats::openOverlayFileSuccessful);
    return file;
  } catch (const std::exception& e) {
    XLOGF(ERR, "Failed to open file {} {}", inodeNumber, e.what());
    stats_->increment(&OverlayStats::openOverlayFileFailure);
    throw;
  }
}

OverlayFile Overlay::createOverlayFile(
    InodeNumber inodeNumber,
    folly::ByteRange contents) {
  DurationScope<EdenStats> statScope{stats_, &OverlayStats::createOverlayFile};
  IORequest req{this};
  try {
    XCHECK_LT(
        inodeNumber.get(), nextInodeNumber_.load(std::memory_order_relaxed))
        << "createOverlayFile called with unallocated inode number";
    XCHECK(fileContentStore_);
    auto file = OverlayFile(
        fileContentStore_->createOverlayFile(inodeNumber, contents),
        weak_from_this());
    stats_->increment(&OverlayStats::createOverlayFileSuccessful);
    return file;
  } catch (const std::exception& e) {
    XLOGF(ERR, "Failed to create file {} {}", inodeNumber, e.what());
    stats_->increment(&OverlayStats::createOverlayFileFailure);
    throw;
  }
}

OverlayFile Overlay::createOverlayFile(
    InodeNumber inodeNumber,
    const folly::IOBuf& contents) {
  DurationScope<EdenStats> statScope{stats_, &OverlayStats::createOverlayFile};
  IORequest req{this};
  try {
    XCHECK_LT(
        inodeNumber.get(), nextInodeNumber_.load(std::memory_order_relaxed))
        << "createOverlayFile called with unallocated inode number";
    XCHECK(fileContentStore_);
    auto file = OverlayFile(
        fileContentStore_->createOverlayFile(inodeNumber, contents),
        weak_from_this());
    stats_->increment(&OverlayStats::createOverlayFileSuccessful);
    return file;
  } catch (const std::exception& e) {
    XLOGF(ERR, "Failed to create file {} {}", inodeNumber, e.what());
    stats_->increment(&OverlayStats::createOverlayFileFailure);
    throw;
  }
}

#endif // !_WIN32

InodeNumber Overlay::getMaxInodeNumber() {
  auto ino = nextInodeNumber_.load(std::memory_order_relaxed);
  XCHECK_GT(ino, 1u);
  return InodeNumber{ino - 1};
}

bool Overlay::tryIncOutstandingIORequests() {
  uint64_t currentOutstandingIO =
      outstandingIORequests_.load(std::memory_order_seq_cst);

  // Retry incrementing the IO count while we have not either successfully
  // updated outstandingIORequests_ or closed the overlay
  while (!(currentOutstandingIO & ioClosedMask)) {
    // If not closed, currentOutstandingIO now holds what
    // outstandingIORequests_ actually contained
    if (outstandingIORequests_.compare_exchange_weak(
            currentOutstandingIO,
            currentOutstandingIO + 1,
            std::memory_order_seq_cst)) {
      return true;
    }
  }

  // If we have broken out of the above loop, the overlay is closed and we
  // been unable to increment outstandingIORequests_.
  return false;
}

void Overlay::decOutstandingIORequests() {
  uint64_t outstanding =
      outstandingIORequests_.fetch_sub(1, std::memory_order_seq_cst);
  XCHECK_NE(0ull, outstanding) << "Decremented too far!";
  // If the overlay is closed and we just finished our last IO request (meaning
  // the previous value of outstandingIORequests_ was 1), then wake the waiting
  // thread.
  if ((outstanding & ioClosedMask) && (outstanding & ioCountMask) == 1) {
    lastOutstandingRequestIsComplete_.post();
  }
}

void Overlay::closeAndWaitForOutstandingIO() {
  uint64_t outstanding =
      outstandingIORequests_.fetch_or(ioClosedMask, std::memory_order_seq_cst);

  // If we have outstanding IO requests, wait for them. This should not block if
  // this baton has already been posted between the load in the fetch_or and
  // this if statement.
  if (outstanding & ioCountMask) {
    lastOutstandingRequestIsComplete_.wait();
  }
}

void Overlay::gcThread() noexcept {
  for (;;) {
    std::vector<GCRequest> requests;
    {
      auto lock = gcQueue_.lock();
      while (lock->queue.empty()) {
        if (lock->stop) {
          return;
        }
        gcCondVar_.wait(lock.as_lock());
      }

      requests = std::move(lock->queue);
    }

    for (auto& request : requests) {
      try {
        handleGCRequest(request);
      } catch (const std::exception& e) {
        XLOGF(
            ERR,
            "handleGCRequest should never throw, but it did: {}",
            e.what());
      }
    }
  }
}

void Overlay::handleGCRequest(GCRequest& request) {
  IORequest req{this};

  if (std::holds_alternative<GCRequest::MaintenanceRequest>(
          request.requestType)) {
    inodeCatalog_->maintenance();
    return;
  }

  if (auto* flush =
          std::get_if<GCRequest::FlushRequest>(&request.requestType)) {
    flush->setValue();
    return;
  }

  // Should only include inode numbers for trees.
  std::queue<InodeNumber> queue;

  // TODO: For better throughput on large tree collections, it might make
  // sense to split this into two threads: one for traversing the tree and
  // another that makes the actual unlink calls.
  auto safeRemoveOverlayFile = [&](InodeNumber inodeNumber) {
    try {
      removeOverlayFile(inodeNumber);
    } catch (const std::exception& e) {
      XLOGF(
          ERR,
          "Failed to remove overlay data for file inode {}: {}",
          inodeNumber,
          e.what());
    }
  };

  auto processDir = [&](const overlay::OverlayDir& dir) {
    for (const auto& entry : *dir.entries()) {
      const auto& value = entry.second;
      if (!(*value.inodeNumber())) {
        // Legacy-only.  All new Overlay trees have inode numbers for all
        // children.
        continue;
      }
      auto ino = InodeNumber::fromThrift(*value.inodeNumber());

      if (S_ISDIR(*value.mode())) {
        queue.push(ino);
      } else {
        // No need to recurse, but delete any file at this inode.  Note that,
        // under normal operation, there should be nothing at this path
        // because files are only written into the overlay if they're
        // materialized.
        safeRemoveOverlayFile(ino);
      }
    }
  };

  processDir(std::get<overlay::OverlayDir>(request.requestType));

  while (!queue.empty()) {
    auto ino = queue.front();
    queue.pop();

    overlay::OverlayDir dir;
    try {
      freeInodeFromMetadataTable(ino);
      auto dirData = inodeCatalog_->loadAndRemoveOverlayDir(ino);
      if (!dirData.has_value()) {
        XLOGF(DBG7, "no dir data for inode {}", ino);
        continue;
      } else {
        dir = std::move(*dirData);
      }
    } catch (const std::exception& e) {
      XLOGF(
          ERR,
          "While collecting, failed to load tree data for inode {}: {}",
          ino,
          e.what());
      continue;
    }

    processDir(dir);
  }
}

void Overlay::addChild(
    InodeNumber parent,
    const std::pair<PathComponent, DirEntry>& childEntry,
    const DirContents& content) {
  DurationScope<EdenStats> statScope{stats_, &OverlayStats::addChild};
  try {
    if (supportsSemanticOperations_) {
      inodeCatalog_->addChild(
          parent, childEntry.first, serializeOverlayEntry(childEntry.second));
    } else {
      saveOverlayDir(parent, content);
    }
    stats_->increment(&OverlayStats::addChildSuccessful);
  } catch (const std::exception& e) {
    XLOGF(ERR, "Failed to add child {} {}", childEntry.first, e.what());
    stats_->increment(&OverlayStats::addChildFailure);
    throw;
  }
}

void Overlay::removeChild(
    InodeNumber parent,
    PathComponentPiece childName,
    const DirContents& content) {
  DurationScope<EdenStats> statScope{stats_, &OverlayStats::removeChild};
  try {
    if (supportsSemanticOperations_) {
      if (inodeCatalog_->removeChild(parent, childName)) {
        stats_->increment(&OverlayStats::removeChildSuccessful);
      }
    } else {
      saveOverlayDir(parent, content);
      stats_->increment(&OverlayStats::removeChildSuccessful);
    }
  } catch (const std::exception& e) {
    XLOGF(ERR, "Failed to remove child {} {}", childName, e.what());
    stats_->increment(&OverlayStats::removeChildFailure);
    throw;
  }
}

void Overlay::removeChildren(InodeNumber parent, const DirContents& content) {
  DurationScope<EdenStats> statScope{stats_, &OverlayStats::removeChildren};
  try {
    saveOverlayDir(parent, content);
    stats_->increment(&OverlayStats::removeChildrenSuccessful);
  } catch (const std::exception& e) {
    XLOGF(ERR, "Failed to remove children {}", e.what());
    stats_->increment(&OverlayStats::removeChildrenFailure);
    throw;
  }
}

void Overlay::renameChild(
    InodeNumber src,
    InodeNumber dst,
    PathComponentPiece srcName,
    PathComponentPiece dstName,
    const DirContents& srcContent,
    const DirContents& dstContent) {
  DurationScope<EdenStats> statScope{stats_, &OverlayStats::renameChild};
  try {
    if (supportsSemanticOperations_) {
      inodeCatalog_->renameChild(src, dst, srcName, dstName);
    } else {
      saveOverlayDir(src, srcContent);
      if (dst.get() != src.get()) {
        saveOverlayDir(dst, dstContent);
      }
    }
    stats_->increment(&OverlayStats::renameChildSuccessful);
  } catch (const std::exception& e) {
    XLOGF(ERR, "Failed to rename child {} {}", srcName, e.what());
    stats_->increment(&OverlayStats::renameChildFailure);
    throw;
  }
}

void Overlay::maintenance() {
  gcQueue_.lock()->queue.emplace_back(Overlay::GCRequest::MaintenanceRequest{});
}
} // namespace facebook::eden
