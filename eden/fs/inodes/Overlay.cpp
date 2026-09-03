/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/Overlay.h"

#include <boost/filesystem.hpp>
#include <algorithm>
#include <array>
#include <charconv>

#ifndef _WIN32
#include <fcntl.h>
#endif

#include <fmt/format.h>
#include <folly/Exception.h>
#include <folly/File.h>
#include <folly/FileUtil.h>
#include <folly/Random.h>
#include <folly/Range.h>
#include <folly/ScopeGuard.h>
#include <folly/io/IOBuf.h>
#include <folly/logging/xlog.h>
#include <folly/stop_watch.h>
#include <folly/system/ThreadName.h>
#include <thrift/lib/cpp2/protocol/Serializer.h>

#include "eden/common/telemetry/DurationScope.h"
#include "eden/common/utils/Bug.h"
#include "eden/common/utils/PathFuncs.h"
#include "eden/common/utils/PathMapMutator.h"
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
#include "eden/fs/telemetry/EdenErrorInfoBuilder.h"
#include "eden/fs/telemetry/EdenFsEventsLogger.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/ErrorLogger.h"
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

#ifndef _WIN32
constexpr folly::StringPiece kInodeReservationFile{"inode-reservation"};
constexpr folly::StringPiece kInodeReservationTempFile{"inode-reservation.tmp"};
constexpr uint64_t kInodeReservationSize = 4096;

// Load on-disk inode reservation state. Returns nullopt on unexpected on-disk
// state.
std::optional<uint64_t> loadInodeReservation(AbsolutePathPiece localDir) {
  const auto path = localDir + PathComponentPiece{kInodeReservationFile};
  std::array<char, 32> target{};
  const auto size = ::readlink(path.c_str(), target.data(), target.size());
  if (size < 0) {
    if (errno == ENOENT) {
      return std::nullopt;
    }
    XLOGF(
        WARN,
        "Failed to read inode reservation {}: {}",
        path,
        folly::errnoStr(errno));
    return std::nullopt;
  }

  uint64_t reservation;
  const auto result = std::from_chars(
      target.data(), target.data() + size, reservation, /*base=*/10);
  if (result.ec != std::errc{} || result.ptr != target.data() + size ||
      reservation <= kRootNodeId.get()) {
    XLOGF(WARN, "Invalid inode reservation in {}", path);
    return std::nullopt;
  }
  return reservation;
}
#endif

bool getOverlayEntryIsRestricted(const overlay::OverlayEntry& entry) {
  return apache::thrift::is_non_optional_field_set_manually_or_by_serializer(
             entry.isRestricted())
      ? *entry.isRestricted()
      : false;
}

AclRootState getOverlayEntryAclRootState(const overlay::OverlayEntry& entry) {
  if (auto state = entry.aclRootState()) {
    if (auto aclRootState = aclRootStateFromInt(*state)) {
      return *aclRootState;
    }
    XLOGF(
        WARN,
        "Invalid overlay ACL root state {}; falling back to legacy isRestricted",
        *state);
  }
  return makeAclRootState(
      getOverlayEntryIsRestricted(entry), std::optional<bool>{std::nullopt});
}

std::unique_ptr<InodeCatalog> makeInodeCatalog(
    AbsolutePathPiece localDir,
    InodeCatalogType inodeCatalogType,
    InodeCatalogOptions inodeCatalogOptions,
    const EdenConfig& config,
    FileContentStore* fileContentStore,
    const std::shared_ptr<EdenFsEventsLogger>& logger) {
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
  if (inodeCatalogType == InodeCatalogType::Legacy ||
      inodeCatalogType == InodeCatalogType::LegacyDev ||
      inodeCatalogType == InodeCatalogType::LegacyEphemeral) {
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
  if (inodeCatalogType == InodeCatalogType::LegacyEphemeral) {
    XLOG(
        WARN,
        "Ephemeral legacy overlay being used. This will cause data loss.");
    return std::make_unique<EphemeralFsInodeCatalog>(
        static_cast<FsFileContentStore*>(fileContentStore));
  }
  XLOG(DBG4, "Legacy overlay being used.");
  return std::make_unique<FsInodeCatalog>(
      static_cast<FsFileContentStore*>(fileContentStore));
#endif
}

std::unique_ptr<FileContentStore> makeFileContentStore(
    AbsolutePathPiece localDir,
    const std::shared_ptr<EdenFsEventsLogger>& logger,
    InodeCatalogType inodeCatalogType,
    bool directFileCreate,
    bool cacheWalFiles) {
#ifdef _WIN32
  (void)localDir;
  (void)logger;
  (void)directFileCreate;
  (void)cacheWalFiles;
  return nullptr;
#else
  // LegacyEphemeral only applies to the inode catalog, not the file content
  // store
  if (inodeCatalogType == InodeCatalogType::Legacy ||
      inodeCatalogType == InodeCatalogType::LegacyEphemeral) {
    return std::make_unique<FsFileContentStore>(
        localDir, directFileCreate, cacheWalFiles);
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
    std::shared_ptr<EdenFsEventsLogger> logger,
    ErrorLogger& errorLogger,
    EdenStatsPtr stats,
    const EdenConfig& config) {
  // This allows us to access the private constructor.
  struct MakeSharedEnabler : public Overlay {
    explicit MakeSharedEnabler(
        AbsolutePathPiece localDir,
        CaseSensitivity caseSensitive,
        InodeCatalogType inodeCatalogType,
        InodeCatalogOptions inodeCatalogOptions,
        std::shared_ptr<EdenFsEventsLogger> logger,
        ErrorLogger& errorLogger,
        EdenStatsPtr stats,
        const EdenConfig& config)
        : Overlay(
              localDir,
              caseSensitive,
              inodeCatalogType,
              inodeCatalogOptions,
              logger,
              errorLogger,
              std::move(stats),
              config) {}
  };
  return std::make_shared<MakeSharedEnabler>(
      localDir,
      caseSensitive,
      inodeCatalogType,
      inodeCatalogOptions,
      logger,
      errorLogger,
      std::move(stats),
      config);
}

Overlay::Overlay(
    AbsolutePathPiece localDir,
    CaseSensitivity caseSensitive,
    InodeCatalogType inodeCatalogType,
    InodeCatalogOptions inodeCatalogOptions,
    std::shared_ptr<EdenFsEventsLogger> logger,
    ErrorLogger& errorLogger,
    EdenStatsPtr stats,
    const EdenConfig& config)
    : fileContentStore_{makeFileContentStore(
          localDir,
          logger,
          inodeCatalogType,
          config.experimentalOverlayDirectFileCreate.getValue(),
          config.experimentalOverlayCacheWalFiles.getValue())},
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
      edenFsEventsLogger_{std::move(logger)},
      errorLogger_(errorLogger),
      stats_{std::move(stats)},
      useDirectFileWrites_(config.overlayDirectFileWrites.getValue()),
      useInodeReservation_{
          !folly::kIsWindows && config.overlayInodeReservation.getValue() &&
          (inodeCatalogType == InodeCatalogType::Legacy ||
           inodeCatalogType == InodeCatalogType::LegacyDev)},
      useWal_{config.overlayUseWal.getValue() && inodeCatalog_->supportsWal()},
      walCompactionMultiplier_{
          config.overlayWalCompactionMultiplier.getValue()},
      walCompactionByteCap_{config.overlayWalCompactionByteCap.getValue()},
      walMinCompactionThreshold_{static_cast<size_t>(
          config.experimentalOverlayWalMinCompactionThreshold.getValue())},
      walCompactionRng_{[] { return folly::Random::rand32(); }} {
#ifndef _WIN32
  if (fileContentStore_) {
    filePreallocPoolSize_ = config.overlayFilePreallocPoolSize.getValue();
  }
  // Only the Fs catalog pays a file creation (tmp file + rename) per dir
  // record; DB-backed catalogs write records cheaply and would only gain
  // orphan rows from preallocation.
  if (inodeCatalogType == InodeCatalogType::Legacy ||
      inodeCatalogType == InodeCatalogType::LegacyDev ||
      inodeCatalogType == InodeCatalogType::LegacyEphemeral) {
    dirPreallocPoolSize_ = config.overlayDirPreallocPoolSize.getValue();
  }
#endif
}

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

#ifndef _WIN32
  stopPreallocThread();
#endif

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
  // TODO(helsel): if SQLiteInodeCatalog maintained a pointer to the
  // fileContentStore, it could call close() on the core in its own close()
  // method. We could get rid of this codeblock in that case.
  if (fileContentStore_ && inodeCatalogType_ != InodeCatalogType::Legacy &&
      inodeCatalogType_ != InodeCatalogType::LegacyDev &&
      inodeCatalogType_ != InodeCatalogType::LegacyEphemeral) {
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
    const std::shared_ptr<ReloadableConfig>& config,
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
                           config,
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
#ifndef _WIN32
    if (filePreallocPoolSize_ > 0 || dirPreallocPoolSize_ > 0) {
      preallocThread_ = std::thread([this] { preallocThreadLoop(); });
    }
#endif

    promise.setValue();

    gcThread();
  });
  return std::move(initFuture);
}

void Overlay::initOverlay(
    std::shared_ptr<ReloadableConfig> config,
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
    // Alternatively, if SqliteInodeCatalog maintained a pointer to the
    // fileContentStore, it could call `initialize` on the fileContentStore in
    // its own `initialize` method and also make this code block unnecessary.
    // TODO(helsel): teach SqliteInodeCatalog to manage its own fileContentStore
    fileContentStore_->initialize(/*createIfNonExisting=*/true);
  }
  if (!optNextInodeNumber.has_value()) {
#ifndef _WIN32
    // FSCK is not currently supported for LMDB overlays. If we cannot load the
    // next inode number, then we cannot continue. LMDB should always be able to
    // load the inode number, if this case is hit, then the assumption about
    // LMDB being resilient is incorrect (unless the user manually corrupted
    // their overlay directory).
    if (inodeCatalogType_ == InodeCatalogType::LMDB) {
      throw std::runtime_error(
          "Corrupted LMDB overlay " + localDir_.asString() +
          ": could not load next inode number");
    }

    if (inodeCatalogType_ == InodeCatalogType::LegacyEphemeral) {
      throw std::runtime_error(
          "EphemeralFsInodeCatalog::initOverlay should either throw or always "
          "return a nextInodeNumber, but optNextInodeNumber is missing");
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

    // Limit concurrent fsck operations to prevent OOM when many mounts
    // need fsck after ungraceful shutdown.
    if (fsckSemaphore_) {
      if (progressCallback) {
        progressCallback(
            OverlayChecker::Progress{
                OverlayChecker::Progress::Phase::WaitingForFsckSlot});
      }
      if (preFsckSemaphoreCallback_) {
        preFsckSemaphoreCallback_();
      }
      folly::stop_watch<std::chrono::milliseconds> waitTimer;
      XLOGF(DBG2, "Overlay {}: waiting for fsck slot", localDir_);
      fsckSemaphore_->wait();
      XLOGF(
          DBG2,
          "Overlay {}: acquired fsck slot after {}ms",
          localDir_,
          waitTimer.elapsed().count());
      if (progressCallback) {
        progressCallback(
            OverlayChecker::Progress{
                OverlayChecker::Progress::Phase::FsckSlotAcquired});
      }
    }
    SCOPE_EXIT {
      if (fsckSemaphore_) {
        fsckSemaphore_->post();
      }
    };

    if (fsckCallback_) {
      fsckCallback_();
    }

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
        config->getEdenConfig()->fsckNumErrorDiscoveryThreads.getValue(),
        caseSensitive_,
        config->getEdenConfig()->fsckUseMemoryEfficientScan.getValue());
    folly::stop_watch<> fsckRuntime;
    auto result = checker.repairErrors(progressCallback);
    auto fsckRuntimeInSeconds =
        std::chrono::duration<double>{fsckRuntime.elapsed()}.count();
    if (result) {
      // If totalErrors - fixedErrors is nonzero, then we failed to
      // fix all of the problems.
      auto success = !(result->totalErrors - result->fixedErrors);
      edenFsEventsLogger_->logEvent(
          Fsck{fsckRuntimeInSeconds, success, true /*attempted_repair*/});
    } else {
      edenFsEventsLogger_->logEvent(
          Fsck{
              fsckRuntimeInSeconds,
              true /*success*/,
              false /*attempted_repair*/});
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
        std::move(config), *mountPath, lookupCallback);
    auto fsckRuntimeInSeconds =
        std::chrono::duration<double>{fsckRuntime.elapsed()}.count();
    edenFsEventsLogger_->logEvent(
        Fsck{
            fsckRuntimeInSeconds,
            true /*success*/,
            false /*attempted_repair*/});
  }

  auto nextInodeNumber = optNextInodeNumber->get();
#ifndef _WIN32
  if (useInodeReservation_) {
    initializeInodeReservation(nextInodeNumber);
  }
#endif
  nextInodeNumber_.store(nextInodeNumber, std::memory_order_relaxed);

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
#ifndef _WIN32
  if (useInodeReservation_) {
    ensureInodeReservation(previous + 1);
  }
#endif
  return InodeNumber{previous};
}

InodeNumber Overlay::allocateInodeNumbers(uint64_t count) {
  static_assert(
      sizeof(nextInodeNumber_) == sizeof(InodeNumber),
      "expected nextInodeNumber_ and InodeNumber to have the same size");
  static_assert(
      sizeof(InodeNumber) >= 8, "expected InodeNumber to be at least 64 bits");

  auto previous = nextInodeNumber_.fetch_add(count);
  XDCHECK_NE(0u, previous) << "allocateInodeNumbers called before initialize";
#ifndef _WIN32
  if (useInodeReservation_ && count != 0) {
    ensureInodeReservation(previous + count);
  }
#endif
  return InodeNumber{previous};
}

#ifndef _WIN32
void Overlay::initializeInodeReservation(uint64_t nextInodeNumber) {
  auto reservation = loadInodeReservation(localDir_);
  // Inode reservation should be >= nextInodeNumber.
  if (!reservation || *reservation <= nextInodeNumber) {
    reservation = nextInodeNumber + kInodeReservationSize;
    saveInodeReservation(*reservation);
  }
  inodeReservation_.store(*reservation, std::memory_order_release);
}

void Overlay::ensureInodeReservation(uint64_t allocatedEnd) {
  auto reservation = inodeReservation_.load(std::memory_order_acquire);
  if (allocatedEnd <= reservation) {
    return;
  }

  auto reservationUpdateLock = inodeReservationUpdateLock_.lock();
  reservation = inodeReservation_.load(std::memory_order_relaxed);
  if (allocatedEnd <= reservation) {
    return;
  }

  auto reservationsNeeded =
      (allocatedEnd - reservation) / kInodeReservationSize + 1;
  reservation += reservationsNeeded * kInodeReservationSize;
  saveInodeReservation(reservation);
  inodeReservation_.store(reservation, std::memory_order_release);
}

// Update on-disk inode reservation state. Throws on I/O error.
void Overlay::saveInodeReservation(uint64_t reservation) {
  const auto path = localDir_ + PathComponentPiece{kInodeReservationFile};
  const auto tempPath =
      localDir_ + PathComponentPiece{kInodeReservationTempFile};
  if (::unlink(tempPath.c_str()) != 0 && errno != ENOENT) {
    folly::throwSystemError(
        "Failed to remove temporary inode reservation ", tempPath.view());
  }

  SCOPE_FAIL {
    ::unlink(tempPath.c_str());
  };

  const auto target = fmt::format("{}", reservation);
  folly::checkUnixError(
      ::symlink(target.c_str(), tempPath.c_str()),
      "Failed to create temporary inode reservation ",
      tempPath.view());
  folly::checkUnixError(
      ::rename(tempPath.c_str(), path.c_str()),
      "Failed to replace inode reservation ",
      path.view());

  folly::File directory{localDir_.c_str(), O_RDONLY | O_DIRECTORY | O_CLOEXEC};
#ifdef __APPLE__
  const auto fsyncResult = ::fcntl(directory.fd(), F_FULLFSYNC);
#else
  const auto fsyncResult = ::fsync(directory.fd());
#endif
  folly::checkUnixError(
      fsyncResult, "Failed to persist inode reservation in ", localDir_.view());
}
#endif

bool Overlay::buildDirEntries(
    OverlayEntrySource source,
    folly::fbvector<std::pair<PathComponent, DirEntry>>& entries) {
  bool shouldRewriteOverlay = false;

  source([&](const std::string& name, const overlay::OverlayEntry& value) {
    if (filterAppleDouble_ && string_view{name}.starts_with("._")) {
      shouldRewriteOverlay = true;
      return;
    }

    InodeNumber ino;
    if (*value.inodeNumber()) {
      ino = InodeNumber::fromThrift(*value.inodeNumber());
    } else {
      ino = allocateInodeNumber();
      shouldRewriteOverlay = true;
    }

    const auto aclRootState = getOverlayEntryAclRootState(value);
    if (value.hash() && !value.hash()->empty()) {
      auto hash = ObjectId{folly::ByteRange{folly::StringPiece{*value.hash()}}};
      entries.emplace_back(
          PathComponent{name},
          DirEntry{
              static_cast<mode_t>(*value.mode()), ino, hash, aclRootState});
    } else {
      entries.emplace_back(
          PathComponent{name},
          DirEntry{static_cast<mode_t>(*value.mode()), ino, aclRootState});
    }
  });

  return shouldRewriteOverlay;
}

DirContents Overlay::loadOverlayDir(InodeNumber inodeNumber) {
  DurationScope<EdenStats> statScope{stats_, &OverlayStats::loadOverlayDir};
  IORequest req{this};
  folly::fbvector<std::pair<PathComponent, DirEntry>> entries;
  bool shouldRewriteOverlay = false;

  bool hasWal = false;
  if (canHaveWalFiles()) {
    hasWal = inodeCatalog_->hasWal(inodeNumber);
  }

  bool found = inodeCatalog_->loadOverlayEntries(
      inodeNumber,
      [&](uint32_t count, InodeCatalog::OverlayEntryIterator iterate) {
        entries.reserve(count);
        shouldRewriteOverlay = buildDirEntries(iterate, entries);
      });
  if (!found && !hasWal) {
    stats_->increment(&OverlayStats::loadOverlayDirFailure);
    return DirContents{caseSensitive_};
  }
  if (!found && hasWal) {
    // Base file is missing but a WAL exists. This happens when the
    // daemon crashed between appendWalEntry creating the WAL and the
    // first saveOverlayDir creating the base — or when the base was
    // truncated/lost externally. Replay the WAL onto an empty base
    // rather than dropping it (the WAL ADDs reference real on-disk
    // inodes that would otherwise become orphans for fsck to delete).
    XLOGF(
        WARN,
        "Overlay base missing for inode {} but WAL present; "
        "replaying WAL onto empty base",
        inodeNumber);
  }

  if (hasWal) {
    // Pre-process WAL into a collapsed net delta and merge it into the
    // streamed-load PathMap via PathMapMutator. saveOverlayDir below
    // flushes the merged base file and clearWalAfterFullWrite removes
    // the WAL.
    auto walResult = inodeCatalog_->loadWalDelta(inodeNumber, caseSensitive_);
    auto& delta = walResult.delta;
    stats_->increment(&OverlayStats::walReplay);
    stats_->increment(
        &OverlayStats::walEntriesReplayed,
        static_cast<double>(walResult.rawEntriesParsed));
    if (walResult.parseErrors > 0) {
      stats_->increment(
          &OverlayStats::walParseFailure,
          static_cast<double>(walResult.parseErrors));
    }

    DirContents base{std::move(entries), caseSensitive_};
    PathMapMutator<DirEntry> mutator{std::move(base)};

    for (auto& [name, walDelta] : delta) {
      switch (walDelta.type) {
        case WalOpType::ADD: {
          auto mode = static_cast<mode_t>(*walDelta.entry.mode());
          auto ino = InodeNumber::fromThrift(*walDelta.entry.inodeNumber());
          const auto aclRootState = getOverlayEntryAclRootState(walDelta.entry);
          DirEntry entry{mode, ino, aclRootState};
          if (walDelta.entry.hash().has_value() &&
              !walDelta.entry.hash()->empty()) {
            auto hash = ObjectId{
                folly::ByteRange{folly::StringPiece{*walDelta.entry.hash()}}};
            entry = DirEntry{mode, ino, hash, aclRootState};
          }
          if (caseSensitive_ == CaseSensitivity::Sensitive) {
            // WAL key matches the stored key exactly — no rekey needed, so
            // insert_or_assign is sufficient (vs. the erase+emplace in the
            // case-insensitive branch).
            mutator.insert_or_assign(
                PathComponentPiece{name}, std::move(entry));
          } else {
            // On case-insensitive mounts the stored key spelling may differ
            // from the WAL ADD's spelling (e.g., base "foo" with WAL ADD
            // "FOO"). Erase any case-equivalent entry first so the inserted
            // key uses the WAL casing.
            mutator.erase(PathComponentPiece{name});
            mutator.emplace(PathComponentPiece{name}, std::move(entry));
          }
          break;
        }
        case WalOpType::REMOVE:
          mutator.erase(PathComponentPiece{name});
          break;
        case WalOpType::MATERIALIZE: {
          auto it = mutator.find(PathComponentPiece{name});
          if (it != mutator.end()) {
            it->second.setMaterialized();
          }
          break;
        }
      }
    }

    DirContents merged{mutator.finalize()};
    if (!shouldDeferWalFlush() || walResult.parseErrors > 0 ||
        shouldRewriteOverlay) {
      // The clean-WAL rewrite is opportunistic compaction, redundant with what
      // saveOverlayPostCheckout does, so it's deferred during checkout. But a
      // torn WAL (parseErrors) needs its heal now, and a legacy base
      // (shouldRewriteOverlay: freshly allocated inode numbers or filtered
      // AppleDouble entries) needs to be persisted now -- neither is guaranteed
      // to reach a post-checkout save for directories the checkout only reads.
      saveOverlayDir(inodeNumber, merged, /*isMaterialized=*/true);
    }
    stats_->increment(&OverlayStats::loadOverlayDirSuccessful);
    return merged;
  }

  DirContents result{std::move(entries), caseSensitive_};

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
  entry.isRestricted() = ent.isRestricted();
  entry.aclRootState() = static_cast<int32_t>(ent.aclRootState());

  return entry;
}

void Overlay::visitDirEntries(
    InodeNumber inodeNumber,
    const DirContents& dir,
    OverlayEntryVisitor visitor) {
  auto nextInodeNumber = nextInodeNumber_.load(std::memory_order_relaxed);
  XCHECK_LT(inodeNumber.get(), nextInodeNumber)
      << "visitDirEntries called with unallocated inode number";

  for (const auto& [entName, ent] : dir) {
    XCHECK_NE(entName, "") << fmt::format(
        "visitDirEntries called with entry with an empty path for directory with inodeNumber={}",
        inodeNumber);
    XCHECK_LT(ent.getInodeNumber().get(), nextInodeNumber)
        << "visitDirEntries called with entry using unallocated inode number";

    visitor(entName.asString(), serializeOverlayEntry(ent));
  }
}

overlay::OverlayDir Overlay::serializeOverlayDir(
    InodeNumber inodeNumber,
    const DirContents& dir) {
  IORequest req{this};

  // TODO: T20282158 clean up access of child inode information.
  //
  // Translate the data to the thrift equivalents
  overlay::OverlayDir odir;

  visitDirEntries(
      inodeNumber,
      dir,
      [&](const std::string& name, const overlay::OverlayEntry& entry) {
        odir.entries()->emplace(name, entry);
      });

  return odir;
}

void Overlay::saveOverlayDir(
    InodeNumber inodeNumber,
    const DirContents& dir,
    bool isMaterialized) {
  DurationScope<EdenStats> statScope{stats_, &OverlayStats::saveOverlayDir};
  IORequest req{this};

  // Set crashSafe=false If config flag is enabled and the directory is _not_
  // materialized. Non-materialized directories match source control, so are not
  // "precious" data. crashSafe=false causes the FsInodeCatalog to skip the temp
  // file + rename, instead writing directly to the overlay file.
  bool crashSafe = isMaterialized || !useDirectFileWrites_;

  try {
    inodeCatalog_->saveOverlayEntries(
        inodeNumber,
        dir.size(),
        [&](OverlayEntryVisitor visitor) {
          visitDirEntries(inodeNumber, dir, visitor);
        },
        crashSafe);
    stats_->increment(&OverlayStats::saveOverlayDirSuccessful);
  } catch (const std::exception& e) {
    XLOGF(ERR, "Failed to save overlay dir {} {}", inodeNumber, e.what());
    errorLogger_.log(EdenErrorInfo::overlay(e, inodeNumber.get()));
    stats_->increment(&OverlayStats::saveOverlayDirFailure);
    throw;
  }

  // Any pending WAL entries are now redundant: the base file we just wrote
  // already reflects the in-memory state.
  clearWalAfterFullWrite(inodeNumber);
}

void Overlay::clearWalAfterFullWrite(InodeNumber parent) {
  if (!canHaveWalFiles()) {
    return;
  }
  // Cleanup failure on a successful base rewrite is best-effort: the
  // base file is durable, so the new state is correct on disk; a stale
  // WAL will be re-merged (idempotently) on the next load. Swallow the
  // error rather than propagate, because the caller is interpreting a
  // throw here as "the save failed" and would mark the dir dirty again.
  try {
    inodeCatalog_->removeWal(parent);
  } catch (const std::exception& ex) {
    XLOGF(
        WARN,
        "removeWal({}) failed after successful base rewrite: {}",
        parent,
        ex.what());
  }
}

void Overlay::mergeWalIntoOverlayDir(
    InodeNumber parent,
    overlay::OverlayDir& dir) {
  if (!canHaveWalFiles() || !inodeCatalog_->hasWal(parent)) {
    return;
  }
  auto walResult = inodeCatalog_->replayWal(parent, dir, caseSensitive_);
  stats_->increment(&OverlayStats::walReplay);
  stats_->increment(
      &OverlayStats::walEntriesReplayed,
      static_cast<double>(walResult.rawEntriesParsed));
  if (walResult.parseErrors > 0) {
    stats_->increment(
        &OverlayStats::walParseFailure,
        static_cast<double>(walResult.parseErrors));
  }
  // Drop the WAL file now that we've folded its entries into `dir`.
  // Callers (recursivelyRemoveOverlayDir) have already removed the base
  // overlay file via loadAndRemoveOverlayDir, so leaving the WAL behind
  // would orphan it on disk until fsck swept it up. removeWal is
  // best-effort; mismatched on-disk state is not worse than the
  // pre-existing pattern (fsck handles it).
  try {
    inodeCatalog_->removeWal(parent);
  } catch (const std::exception& ex) {
    XLOGF(WARN, "removeWal({}) after merge failed: {}", parent, ex.what());
  }
}

void Overlay::maybeCompactWal(
    InodeNumber parent,
    const DirContents& content,
    uint64_t walFileSizeBytes) {
  if (!canHaveWalFiles()) {
    return;
  }
  // Hard cap first: the on-disk WAL byte size is the source of truth.
  bool atCap = walFileSizeBytes >= walCompactionByteCap_;
  if (!atCap) {
    // Below the cap: probabilistic roll. Expected appends between
    // compactions = `threshold`, so amortized rewrite cost is O(1) per
    // append.
    size_t threshold = walMinCompactionThreshold_
        ? std::max(
              walMinCompactionThreshold_,
              walCompactionMultiplier_ * content.size())
        : walCompactionMultiplier_ *
            std::max(content.size(), static_cast<size_t>(10));
    if (walCompactionRng_() % threshold != 0) {
      return;
    }
  }
  stats_->increment(&OverlayStats::walCompaction);
  XLOGF(
      DBG2,
      "Compacting WAL for overlay dir {}; content size {}, wal bytes {} ({})",
      parent,
      content.size(),
      walFileSizeBytes,
      atCap ? "hard cap" : "probabilistic");
  // Pass isMaterialized=true: WAL-tracked dirs are materialized, and we
  // need the crash-safe rename path so a crash mid-rewrite cannot leave
  // a truncated base file alongside a stale WAL.
  DurationScope<EdenStats> compactScope{
      stats_, &OverlayStats::walCompactionInline};
  saveOverlayDir(parent, content, /*isMaterialized=*/true);
}

void Overlay::appendWalEntryAndCompact(
    InodeNumber parent,
    WalOpType op,
    PathComponentPiece childName,
    const overlay::OverlayEntry* entry,
    const DirContents& content) {
  uint64_t walFileSizeBytes =
      inodeCatalog_->appendWalEntry(parent, op, childName, entry);
  stats_->increment(&OverlayStats::walAppend);
  maybeCompactWal(parent, content, walFileSizeBytes);
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

    clearWalAfterFullWrite(inodeNumber);
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

    // This inode's data must be removed from the overlay before
    // recursivelyRemoveOverlayDir returns to avoid a race condition if
    // recursivelyRemoveOverlayDir(I) is called immediately prior to
    // saveOverlayDir(I).  There's also no risk of violating our durability
    // guarantees if the process dies after this call but before the thread
    // could remove this data.
    auto dirData = inodeCatalog_->loadAndRemoveOverlayDir(inodeNumber);
    if (dirData) {
      // Apply any pending WAL entries so the GC walk below enumerates
      // (and deletes) every child the WAL added since the base file was
      // last rewritten. Without this, WAL-only children leak as orphan
      // overlay files on disk.
      mergeWalIntoOverlayDir(inodeNumber, *dirData);
      freeInodeFromMetadataTable(inodeNumber);
      gcQueue_.lock()->queue.emplace_back(std::move(*dirData));
      gcCondVar_.notify_one();
      stats_->increment(&OverlayStats::recursivelyRemoveOverlayDirSuccessful);
    }

    clearWalAfterFullWrite(inodeNumber);
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

void Overlay::recursivelyRemoveOverlayDirBackground(InodeNumber inodeNumber) {
  gcQueue_.lock()->queue.emplace_back(inodeNumber);
  gcCondVar_.notify_one();
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
    errorLogger_.log(EdenErrorInfo::overlay(e, inodeNumber.get()));
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
    errorLogger_.log(EdenErrorInfo::overlay(e, inodeNumber.get()));
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
    errorLogger_.log(EdenErrorInfo::overlay(e, inodeNumber.get()));
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
    errorLogger_.log(EdenErrorInfo::overlay(e, inodeNumber.get()));
    stats_->increment(&OverlayStats::createOverlayFileFailure);
    throw;
  }
}

std::optional<std::pair<InodeNumber, OverlayFile>>
Overlay::tryClaimPreparedFile(folly::ByteRange contents) {
  if (filePreallocPoolSize_ == 0) {
    return std::nullopt;
  }
  IORequest req{this};
  PreparedFile prepared;
  bool needRefill;
  {
    std::lock_guard<std::mutex> guard{preallocMutex_};
    if (preallocPool_.empty()) {
      stats_->increment(&OverlayStats::preallocFileMissed);
      if (!preallocBroken_) {
        preallocCondVar_.notify_one();
      }
      return std::nullopt;
    }
    prepared = std::move(preallocPool_.back());
    preallocPool_.pop_back();
    // Only wake the refill thread once half the pool has drained, so a
    // steady stream of creates costs one wakeup and one refill batch per
    // half-pool of claims rather than a futex wake per claim.
    needRefill = preallocPool_.size() <= filePreallocPoolSize_ / 2;
  }
  if (needRefill) {
    preallocCondVar_.notify_one();
  }
  if (!contents.empty()) {
    // The fd is positioned just past the header, so this appends the
    // contents (e.g. a symlink target) in the right place.
    auto written =
        folly::writeFull(prepared.file.fd(), contents.data(), contents.size());
    if (written < 0) {
      // Remove the file so a failed claim doesn't leave a partial
      // payload behind for fsck to report.
      int writeErrno = errno;
      stats_->increment(&OverlayStats::preallocFileClaimFailed);
      try {
        fileContentStore_->removeOverlayFile(prepared.number);
      } catch (const std::exception& ex) {
        XLOGF(
            WARN,
            "failed to remove overlay file {} after a failed claim write: {}",
            prepared.number,
            folly::exceptionStr(ex));
      }
      folly::throwSystemErrorExplicit(
          writeErrno, "failed to write contents to preallocated overlay file");
    }
  }
  stats_->increment(&OverlayStats::preallocFileClaimed);
  return std::make_pair(
      prepared.number, OverlayFile{std::move(prepared.file), weak_from_this()});
}

std::optional<InodeNumber> Overlay::tryClaimPreparedDir() {
  if (dirPreallocPoolSize_ == 0) {
    return std::nullopt;
  }
  InodeNumber number;
  bool needRefill;
  {
    std::lock_guard<std::mutex> guard{preallocMutex_};
    if (preallocDirPool_.empty()) {
      stats_->increment(&OverlayStats::preallocDirMissed);
      preallocCondVar_.notify_one();
      return std::nullopt;
    }
    number = preallocDirPool_.back();
    preallocDirPool_.pop_back();
    // Match the file pool's refill hysteresis: one wakeup per half-pool of
    // claims rather than a futex wake per mkdir.
    needRefill = preallocDirPool_.size() <= dirPreallocPoolSize_ / 2;
  }
  if (needRefill) {
    preallocCondVar_.notify_one();
  }
  stats_->increment(&OverlayStats::preallocDirClaimed);
  return number;
}

void Overlay::preallocThreadLoop() {
  folly::setThreadName("OverlayPrealloc");
  bool backoff = false;
  bool filePoolBroken = false;
  while (true) {
    size_t wantFiles;
    size_t wantDirs;
    {
      std::unique_lock<std::mutex> lock{preallocMutex_};
      if (backoff) {
        preallocCondVar_.wait_for(
            lock, std::chrono::seconds(1), [&] { return preallocStop_; });
        backoff = false;
      } else {
        preallocCondVar_.wait(lock, [&] {
          return preallocStop_ ||
              (!preallocBroken_ &&
               preallocPool_.size() < filePreallocPoolSize_) ||
              preallocDirPool_.size() < dirPreallocPoolSize_;
        });
      }
      if (preallocStop_) {
        return;
      }
      wantFiles =
          preallocBroken_ ? 0 : filePreallocPoolSize_ - preallocPool_.size();
      wantDirs = dirPreallocPoolSize_ - preallocDirPool_.size();
    }
    std::vector<PreparedFile> fileBatch;
    fileBatch.reserve(wantFiles);
    for (size_t i = 0; i < wantFiles; ++i) {
      // If creation fails below, this number stays consumed; the inode
      // number space is 64 bits, so the gap is harmless.
      auto number = allocateInodeNumber();
      try {
        auto created =
            fileContentStore_->createOverlayFile(number, folly::ByteRange{});
        auto* file = std::get_if<folly::File>(&created);
        if (!file) {
          XLOG(
              WARN,
              "overlay file preallocation is not supported by this "
              "FileContentStore; disabling the file pool");
          filePoolBroken = true;
          // The store may have persisted the file even though it did not
          // return an fd (the LMDB store does); remove it so the probe
          // leaves no orphan behind.
          try {
            fileContentStore_->removeOverlayFile(number);
          } catch (const std::exception& removeEx) {
            XLOGF(
                WARN,
                "failed to remove prealloc probe file {}: {}",
                number,
                folly::exceptionStr(removeEx));
          }
          break;
        }
        fileBatch.push_back(PreparedFile{number, std::move(*file)});
      } catch (const std::exception& ex) {
        // Likely ENOSPC or an unusable overlay directory. Keep what we
        // have and retry after a delay instead of spinning.
        XLOGF(
            WARN,
            "overlay file preallocation failed for {}: {}",
            number,
            folly::exceptionStr(ex));
        backoff = true;
        break;
      }
    }
    std::vector<InodeNumber> dirBatch;
    dirBatch.reserve(wantDirs);
    for (size_t i = 0; i < wantDirs; ++i) {
      auto number = allocateInodeNumber();
      try {
        // Publish a complete record before making its inode number claimable.
        inodeCatalog_->saveOverlayEntries(
            number, 0, [](auto) {}, /*crashSafe=*/true);
        dirBatch.push_back(number);
      } catch (const std::exception& ex) {
        XLOGF(
            WARN,
            "overlay dir preallocation failed for {}: {}; retrying after "
            "backoff",
            number,
            folly::exceptionStr(ex));
        // Best-effort cleanup in case the record was partially created.
        try {
          inodeCatalog_->removeOverlayDir(number);
        } catch (const std::exception& removeEx) {
          XLOGF(
              WARN,
              "failed to remove preallocated overlay dir {} after a failed "
              "write: {}",
              number,
              folly::exceptionStr(removeEx));
        }
        backoff = true;
        break;
      }
    }
    bool allPoolsInactive;
    {
      std::lock_guard<std::mutex> guard{preallocMutex_};
      if (filePoolBroken) {
        preallocBroken_ = true;
      }
      for (auto& entry : fileBatch) {
        preallocPool_.push_back(std::move(entry));
      }
      preallocDirPool_.insert(
          preallocDirPool_.end(), dirBatch.begin(), dirBatch.end());
      allPoolsInactive = (filePreallocPoolSize_ == 0 || preallocBroken_) &&
          dirPreallocPoolSize_ == 0;
    }
    if (allPoolsInactive) {
      return;
    }
  }
}

void Overlay::stopPreallocThread() {
  {
    std::lock_guard<std::mutex> guard{preallocMutex_};
    preallocStop_ = true;
  }
  preallocCondVar_.notify_all();
  if (preallocThread_.joinable()) {
    preallocThread_.join();
  }
  // Remove any unclaimed pre-created files so they don't linger as orphans
  // in the overlay directory.
  std::vector<PreparedFile> leftover;
  {
    std::lock_guard<std::mutex> guard{preallocMutex_};
    leftover.swap(preallocPool_);
  }
  for (auto& entry : leftover) {
    entry.file.closeNoThrow();
    try {
      fileContentStore_->removeOverlayFile(entry.number);
    } catch (const std::exception& ex) {
      XLOGF(
          WARN,
          "failed to remove unclaimed preallocated overlay file {}: {}",
          entry.number,
          folly::exceptionStr(ex));
    }
  }
  std::vector<InodeNumber> dirLeftover;
  {
    std::lock_guard<std::mutex> guard{preallocMutex_};
    dirLeftover.swap(preallocDirPool_);
  }
  for (auto number : dirLeftover) {
    try {
      inodeCatalog_->removeOverlayDir(number);
    } catch (const std::exception& ex) {
      XLOGF(
          WARN,
          "failed to remove unclaimed preallocated overlay dir {}: {}",
          number,
          folly::exceptionStr(ex));
    }
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

  if (auto* inodeNumber = std::get_if<InodeNumber>(&request.requestType)) {
    // Background removal request: seed the queue with the root inode number
    // so the BFS loop below handles the load+remove+recurse.
    queue.push(*inodeNumber);
  } else {
    processDir(std::get<overlay::OverlayDir>(request.requestType));
  }

  while (!queue.empty()) {
    auto ino = queue.front();
    queue.pop();

    overlay::OverlayDir dir;
    try {
      auto dirData = inodeCatalog_->loadAndRemoveOverlayDir(ino);
      if (!dirData.has_value()) {
        XLOGF(DBG7, "no dir data for inode {}", ino);
        continue;
      }
      // Same WAL merge as the entry point: ensure WAL-only children are
      // enumerated for cleanup, then drop the WAL file from disk.
      mergeWalIntoOverlayDir(ino, *dirData);
      freeInodeFromMetadataTable(ino);
      dir = std::move(*dirData);
      clearWalAfterFullWrite(ino);
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
    } else if (useWal()) {
      auto entry = serializeOverlayEntry(childEntry.second);
      appendWalEntryAndCompact(
          parent, WalOpType::ADD, childEntry.first, &entry, content);
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
    } else if (useWal()) {
      appendWalEntryAndCompact(
          parent,
          WalOpType::REMOVE,
          childName,
          /*entry=*/nullptr,
          content);
      stats_->increment(&OverlayStats::removeChildSuccessful);
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
    } else if (useWal()) {
      // Fall back to a full rewrite when dstContent does not yet contain
      // the renamed entry — there is nothing concrete for the ADD WAL
      // entry to carry.
      auto dstIt = dstContent.find(dstName);
      if (dstIt == dstContent.end()) {
        saveOverlayDir(src, srcContent);
        if (dst.get() != src.get()) {
          saveOverlayDir(dst, dstContent);
        }
      } else {
        const bool isCaseOnlyRename = src == dst &&
            caseSensitive_ == CaseSensitivity::Insensitive &&
            isPathPieceEqual(srcName, dstName, CaseSensitivity::Insensitive) &&
            !isPathPieceEqual(srcName, dstName, CaseSensitivity::Sensitive);
        auto entry = serializeOverlayEntry(dstIt->second);
        if (isCaseOnlyRename) {
          // On a case-insensitive mount, source and destination are the same
          // logical key. The replacement ADD is the whole update: replay
          // removes the equivalent source spelling before inserting dstName.
          appendWalEntryAndCompact(
              dst, WalOpType::ADD, dstName, &entry, dstContent);
        } else {
          // Order matters: write ADD-to-dst first so a crash between the
          // two appends leaves the entry visible from both `src` and `dst`
          // rather than dropping it entirely. The user observes the rename
          // as incomplete (the source still exists alongside the destination)
          // and can `rm` the unwanted copy to converge the state. The opposite
          // ordering would risk losing the entry permanently. No fsck pass is
          // required to recover.
          //
          // Both appends bump the per-parent compaction counter, so on a
          // same-dir rename (src == dst) the counter ticks twice — matching
          // the two on-disk WAL entries. Without this the counter under-reports
          // by 50% on same-dir renames and the WAL grows to ~2x the intended
          // threshold before compaction fires. If compaction fires after the
          // first append, the second append targets the freshly-rewritten base
          // file with `srcName` already absent (because srcContent reflects the
          // post-rename state); replayWal tolerates REMOVE on a missing name.
          appendWalEntryAndCompact(
              dst, WalOpType::ADD, dstName, &entry, dstContent);
          appendWalEntryAndCompact(
              src,
              WalOpType::REMOVE,
              srcName,
              /*entry=*/nullptr,
              srcContent);
        }
      }
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

void Overlay::materializeChild(
    InodeNumber parent,
    PathComponentPiece childName,
    const DirContents& content) {
  DurationScope<EdenStats> statScope{stats_, &OverlayStats::materializeChild};
  try {
    if (useWal()) {
      appendWalEntryAndCompact(
          parent,
          WalOpType::MATERIALIZE,
          childName,
          /*entry=*/nullptr,
          content);
    } else {
      // WAL disabled — fall back to a full directory write.
      saveOverlayDir(parent, content);
    }
    stats_->increment(&OverlayStats::materializeChildSuccessful);
  } catch (const std::exception& e) {
    XLOGF(ERR, "Failed to materialize child {} {}", childName, e.what());
    stats_->increment(&OverlayStats::materializeChildFailure);
    throw;
  }
}

void Overlay::maintenance() {
  gcQueue_.lock()->queue.emplace_back(Overlay::GCRequest::MaintenanceRequest{});
}
} // namespace facebook::eden
