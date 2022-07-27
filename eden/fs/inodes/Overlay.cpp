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
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/logging/xlog.h>
#include <folly/stop_watch.h>
#include <thrift/lib/cpp2/protocol/Serializer.h>
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/inodes/DirEntry.h"
#include "eden/fs/inodes/InodeBase.h"
#include "eden/fs/inodes/InodeTable.h"
#include "eden/fs/inodes/OverlayFile.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/inodes/treeoverlay/BufferedTreeOverlay.h"
#include "eden/fs/inodes/treeoverlay/TreeOverlay.h"
#include "eden/fs/sqlite/SqliteDatabase.h"
#include "eden/fs/utils/Bug.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook::eden {

namespace {
constexpr uint64_t ioCountMask = 0x7FFFFFFFFFFFFFFFull;
constexpr uint64_t ioClosedMask = 1ull << 63;

std::unique_ptr<IOverlay> makeOverlay(
    AbsolutePathPiece localDir,
    Overlay::OverlayType overlayType,
    const EdenConfig& config) {
  if (overlayType == Overlay::OverlayType::Tree) {
    return std::make_unique<TreeOverlay>(localDir);
  } else if (overlayType == Overlay::OverlayType::TreeInMemory) {
    XLOG(WARN) << "In-memory overlay requested. This will cause data loss.";
    return std::make_unique<TreeOverlay>(
        std::make_unique<SqliteDatabase>(SqliteDatabase::inMemory));
  } else if (overlayType == Overlay::OverlayType::TreeSynchronousOff) {
    return std::make_unique<TreeOverlay>(
        localDir, TreeOverlayStore::SynchronousMode::Off);
  } else if (overlayType == Overlay::OverlayType::TreeBuffered) {
    XLOG(DBG4) << "Buffered tree overlay being used";
    return std::make_unique<BufferedTreeOverlay>(localDir, config);
  }
#ifdef _WIN32
  if (overlayType == Overlay::OverlayType::Legacy) {
    throw std::runtime_error(
        "Legacy overlay type is not supported. Please reclone.");
  }
  return std::make_unique<TreeOverlay>(localDir);
#else
  return std::make_unique<FsOverlay>(localDir);
#endif
}
} // namespace

using folly::Unit;
using std::optional;

std::shared_ptr<Overlay> Overlay::create(
    AbsolutePathPiece localDir,
    CaseSensitivity caseSensitive,
    OverlayType overlayType,
    std::shared_ptr<StructuredLogger> logger,
    const EdenConfig& config) {
  // This allows us to access the private constructor.
  struct MakeSharedEnabler : public Overlay {
    explicit MakeSharedEnabler(
        AbsolutePathPiece localDir,
        CaseSensitivity caseSensitive,
        OverlayType overlayType,
        std::shared_ptr<StructuredLogger> logger,
        const EdenConfig& config)
        : Overlay(localDir, caseSensitive, overlayType, logger, config) {}
  };
  return std::make_shared<MakeSharedEnabler>(
      localDir, caseSensitive, overlayType, logger, config);
}

Overlay::Overlay(
    AbsolutePathPiece localDir,
    CaseSensitivity caseSensitive,
    OverlayType overlayType,
    std::shared_ptr<StructuredLogger> logger,
    const EdenConfig& config)
    : backingOverlay_{makeOverlay(localDir, overlayType, config)},
      supportsSemanticOperations_{
          backingOverlay_->supportsSemanticOperations()},
      caseSensitive_{caseSensitive},
      structuredLogger_{logger} {}

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
  // Cleanup is not necessary if overlay was not initialized
  if (!backingOverlay_->initialized()) {
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

  backingOverlay_->close(optNextInodeNumber);
}

bool Overlay::isClosed() {
  return outstandingIORequests_.load(std::memory_order_acquire) & ioClosedMask;
}

#ifndef _WIN32
struct statfs Overlay::statFs() {
  IORequest req{this};
  return backingOverlay_->statFs();
}
#endif // !_WIN32

folly::SemiFuture<Unit> Overlay::initialize(
    std::optional<AbsolutePath> mountPath,
    OverlayChecker::ProgressCallback&& progressCallback,
    OverlayChecker::LookupCallback&& lookupCallback) {
  // The initOverlay() call is potentially slow, so we want to avoid
  // performing it in the current thread and blocking returning to our caller.
  //
  // We already spawn a separate thread for garbage collection.  It's convenient
  // to simply use this existing thread to perform the initialization logic
  // before waiting for GC work to do.
  auto [initPromise, initFuture] = folly::makePromiseContract<Unit>();

  gcThread_ = std::thread([this,
                           mountPath = std::move(mountPath),
                           progressCallback = std::move(progressCallback),
                           lookupCallback = std::move(lookupCallback),
                           promise = std::move(initPromise)]() mutable {
    try {
      initOverlay(
          std::move(mountPath), progressCallback, std::move(lookupCallback));
    } catch (std::exception& ex) {
      XLOG(ERR) << "overlay initialization failed for "
                << backingOverlay_->getLocalDir() << ": " << ex.what();
      promise.setException(
          folly::exception_wrapper(std::current_exception(), ex));
      return;
    }
    promise.setValue();

    gcThread();
  });
  return std::move(initFuture);
}

void Overlay::initOverlay(
    std::optional<AbsolutePath> mountPath,
    FOLLY_MAYBE_UNUSED const OverlayChecker::ProgressCallback& progressCallback,
    FOLLY_MAYBE_UNUSED OverlayChecker::LookupCallback&& lookupCallback) {
  IORequest req{this};
  auto optNextInodeNumber = backingOverlay_->initOverlay(true);
  if (!optNextInodeNumber.has_value()) {
#ifndef _WIN32
    // If the next-inode-number data is missing it means that this overlay was
    // not shut down cleanly the last time it was used.  If this was caused by a
    // hard system reboot this can sometimes cause corruption and/or missing
    // data in some of the on-disk state.
    //
    // Use OverlayChecker to scan the overlay for any issues, and also compute
    // correct next inode number as it does so.
    XLOG(WARN) << "Overlay " << backingOverlay_->getLocalDir()
               << " was not shut down cleanly.  Performing fsck scan.";

    // TODO(zeyi): `OverlayCheck` should be associated with the specific
    // Overlay implementation. `reinterpret_cast` is a temporary workaround.
    OverlayChecker checker(
        reinterpret_cast<FsOverlay*>(backingOverlay_.get()),
        std::nullopt,
        std::move(lookupCallback));
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
    // TreeOverlay will always return the value of next Inode number, if we
    // end up here - it's a bug.
    EDEN_BUG() << "Tree Overlay is null value for NextInodeNumber";
#endif
  } else {
    hadCleanStartup_ = true;
  }

  // On Windows, we need to scan the state of the repository every time at
  // start up to find any potential changes happened when EdenFS is not
  // running.
  //
  // HACK: ideally we should not have multiple overlay types. However before
  // the migration is done, this is a reliable way to look for `TreeOverlay`.
  //
  // mountPath will be empty during benchmarking so we must check the value
  // here to skip scanning in that case.
  if (supportsSemanticOperations_ && mountPath.has_value()) {
    optNextInodeNumber = dynamic_cast<TreeOverlay*>(backingOverlay_.get())
                             ->scanLocalChanges(*mountPath);
  }

  nextInodeNumber_.store(optNextInodeNumber->get(), std::memory_order_relaxed);

#ifndef _WIN32
  // Open after infoFile_'s lock is acquired because the InodeTable acquires
  // its own lock, which should be released prior to infoFile_.
  inodeMetadataTable_ =
      InodeMetadataTable::open((backingOverlay_->getLocalDir() +
                                PathComponentPiece{FsOverlay::kMetadataFile})
                                   .c_str());
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
  DirContents result(caseSensitive_);
  IORequest req{this};
  auto dirData = backingOverlay_->loadOverlayDir(inodeNumber);
  if (!dirData.has_value()) {
    return result;
  }
  const auto& dir = dirData.value();

  bool shouldMigrateToNewFormat = false;

  for (auto& iter : *dir.entries_ref()) {
    const auto& name = iter.first;
    const auto& value = iter.second;

    InodeNumber ino;
    if (*value.inodeNumber_ref()) {
      ino = InodeNumber::fromThrift(*value.inodeNumber_ref());
    } else {
      ino = allocateInodeNumber();
      shouldMigrateToNewFormat = true;
    }

    if (value.hash_ref() && !value.hash_ref()->empty()) {
      auto hash =
          ObjectId{folly::ByteRange{folly::StringPiece{*value.hash_ref()}}};
      result.emplace(PathComponentPiece{name}, *value.mode_ref(), ino, hash);
    } else {
      // The inode is materialized
      result.emplace(PathComponentPiece{name}, *value.mode_ref(), ino);
    }
  }

  if (shouldMigrateToNewFormat) {
    saveOverlayDir(inodeNumber, result);
  }

  return result;
}

overlay::OverlayEntry Overlay::serializeOverlayEntry(const DirEntry& ent) {
  overlay::OverlayEntry entry;

  // TODO: Eventually, we should only serialize the child entry's dtype into
  // the Overlay. But, as of now, it's possible to create an inode under a
  // tree, serialize that tree into the overlay, then restart Eden. Since
  // writing mode bits into the InodeMetadataTable only occurs when the inode
  // is loaded, the initial mode bits must persist until the first load.
  entry.mode_ref() = ent.getInitialMode();
  entry.inodeNumber_ref() = ent.getInodeNumber().get();
  if (!ent.isMaterialized()) {
    entry.hash_ref() = ent.getHash().asString();
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

    XCHECK_NE(entName, "")
        << "serializeOverlayDir called with entry with an empty path for directory with inodeNumber="
        << inodeNumber;
    XCHECK_LT(ent.getInodeNumber().get(), nextInodeNumber)
        << "serializeOverlayDir called with entry using unallocated inode number";

    odir.entries_ref()->emplace(std::make_pair(
        entName.stringPiece().str(), serializeOverlayEntry(ent)));
  }

  return odir;
}

void Overlay::saveOverlayDir(InodeNumber inodeNumber, const DirContents& dir) {
  backingOverlay_->saveOverlayDir(
      inodeNumber, serializeOverlayDir(inodeNumber, dir));
}

void Overlay::freeInodeFromMetadataTable(InodeNumber ino) {
#ifndef _WIN32
  // TODO: batch request during GC
  getInodeMetadataTable()->freeInode(ino);
#else
  (void)ino;
#endif
}

void Overlay::removeOverlayData(InodeNumber inodeNumber) {
  IORequest req{this};

  freeInodeFromMetadataTable(inodeNumber);
  backingOverlay_->removeOverlayData(inodeNumber);
}

void Overlay::recursivelyRemoveOverlayData(InodeNumber inodeNumber) {
  IORequest req{this};

  // This inode's data must be removed from the overlay before
  // recursivelyRemoveOverlayData returns to avoid a race condition if
  // recursivelyRemoveOverlayData(I) is called immediately prior to
  // saveOverlayDir(I).  There's also no risk of violating our durability
  // guarantees if the process dies after this call but before the thread could
  // remove this data.
  auto dirData = backingOverlay_->loadAndRemoveOverlayDir(inodeNumber);
  if (dirData) {
    gcQueue_.lock()->queue.emplace_back(std::move(*dirData));
    gcCondVar_.notify_one();
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

bool Overlay::hasOverlayData(InodeNumber inodeNumber) {
  IORequest req{this};
  return backingOverlay_->hasOverlayData(inodeNumber);
}

#ifndef _WIN32
// Helper function to open,validate,
// get file pointer of an overlay file
OverlayFile Overlay::openFile(
    InodeNumber inodeNumber,
    folly::StringPiece headerId) {
  IORequest req{this};
  return OverlayFile(
      backingOverlay_->openFile(inodeNumber, headerId), weak_from_this());
}

OverlayFile Overlay::openFileNoVerify(InodeNumber inodeNumber) {
  IORequest req{this};
  return OverlayFile(
      backingOverlay_->openFileNoVerify(inodeNumber), weak_from_this());
}

OverlayFile Overlay::createOverlayFile(
    InodeNumber inodeNumber,
    folly::ByteRange contents) {
  IORequest req{this};
  XCHECK_LT(inodeNumber.get(), nextInodeNumber_.load(std::memory_order_relaxed))
      << "createOverlayFile called with unallocated inode number";
  return OverlayFile(
      backingOverlay_->createOverlayFile(inodeNumber, contents),
      weak_from_this());
}

OverlayFile Overlay::createOverlayFile(
    InodeNumber inodeNumber,
    const folly::IOBuf& contents) {
  IORequest req{this};
  XCHECK_LT(inodeNumber.get(), nextInodeNumber_.load(std::memory_order_relaxed))
      << "createOverlayFile called with unallocated inode number";
  return OverlayFile(
      backingOverlay_->createOverlayFile(inodeNumber, contents),
      weak_from_this());
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
        continue;
      }

      requests = std::move(lock->queue);
    }

    for (auto& request : requests) {
      try {
        handleGCRequest(request);
      } catch (const std::exception& e) {
        XLOG(ERR) << "handleGCRequest should never throw, but it did: "
                  << e.what();
      }
    }
  }
}

void Overlay::handleGCRequest(GCRequest& request) {
  IORequest req{this};

  if (std::holds_alternative<GCRequest::MaintenanceRequest>(
          request.requestType)) {
    backingOverlay_->maintenance();
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
  auto safeRemoveOverlayData = [&](InodeNumber inodeNumber) {
    try {
      removeOverlayData(inodeNumber);
    } catch (const std::exception& e) {
      XLOG(ERR) << "Failed to remove overlay data for inode " << inodeNumber
                << ": " << e.what();
    }
  };

  auto processDir = [&](const overlay::OverlayDir& dir) {
    for (const auto& entry : *dir.entries_ref()) {
      const auto& value = entry.second;
      if (!(*value.inodeNumber_ref())) {
        // Legacy-only.  All new Overlay trees have inode numbers for all
        // children.
        continue;
      }
      auto ino = InodeNumber::fromThrift(*value.inodeNumber_ref());

      if (S_ISDIR(*value.mode_ref())) {
        queue.push(ino);
      } else {
        // No need to recurse, but delete any file at this inode.  Note that,
        // under normal operation, there should be nothing at this path
        // because files are only written into the overlay if they're
        // materialized.
        safeRemoveOverlayData(ino);
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
      auto dirData = backingOverlay_->loadAndRemoveOverlayDir(ino);
      if (!dirData.has_value()) {
        XLOG(DBG7) << "no dir data for inode " << ino;
        continue;
      } else {
        dir = std::move(*dirData);
      }
    } catch (const std::exception& e) {
      XLOG(ERR) << "While collecting, failed to load tree data for inode "
                << ino << ": " << e.what();
      continue;
    }

    processDir(dir);
  }
}

void Overlay::addChild(
    InodeNumber parent,
    const std::pair<PathComponent, DirEntry>& childEntry,
    const DirContents& content) {
  if (supportsSemanticOperations_) {
    backingOverlay_->addChild(
        parent, childEntry.first, serializeOverlayEntry(childEntry.second));
  } else {
    saveOverlayDir(parent, content);
  }
}

void Overlay::removeChild(
    InodeNumber parent,
    PathComponentPiece childName,
    const DirContents& content) {
  if (supportsSemanticOperations_) {
    backingOverlay_->removeChild(parent, childName);
  } else {
    saveOverlayDir(parent, content);
  }
}

void Overlay::removeChildren(InodeNumber parent, const DirContents& content) {
  saveOverlayDir(parent, content);
}

void Overlay::renameChild(
    InodeNumber src,
    InodeNumber dst,
    PathComponentPiece srcName,
    PathComponentPiece dstName,
    const DirContents& srcContent,
    const DirContents& dstContent) {
  if (supportsSemanticOperations_) {
    backingOverlay_->renameChild(src, dst, srcName, dstName);
  } else {
    saveOverlayDir(src, srcContent);
    if (dst.get() != src.get()) {
      saveOverlayDir(dst, dstContent);
    }
  }
}

void Overlay::maintenance() {
  gcQueue_.lock()->queue.emplace_back(Overlay::GCRequest::MaintenanceRequest{});
}
} // namespace facebook::eden
