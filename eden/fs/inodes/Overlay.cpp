/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/inodes/Overlay.h"

#include <boost/filesystem.hpp>
#include <folly/Exception.h>
#include <folly/File.h>
#include <folly/FileUtil.h>
#include <folly/Range.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/logging/xlog.h>
#include <thrift/lib/cpp2/protocol/Serializer.h>
#include <algorithm>
#include "eden/fs/inodes/DirEntry.h"
#include "eden/fs/inodes/InodeTable.h"
#include "eden/fs/inodes/overlay/FsOverlay.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook {
namespace eden {

using apache::thrift::CompactSerializer;
using folly::ByteRange;
using folly::fbvector;
using folly::File;
using folly::IOBuf;
using folly::MutableStringPiece;
using folly::StringPiece;
using folly::Unit;
using std::optional;
using folly::literals::string_piece_literals::operator""_sp;
using std::string;

Overlay::Overlay(AbsolutePathPiece localDir) : fsOverlay_{localDir} {}

Overlay::~Overlay() {
  close();
}

void Overlay::close() {
  CHECK_NE(std::this_thread::get_id(), gcThread_.get_id());

  gcQueue_.lock()->stop = true;
  gcCondVar_.notify_one();
  if (gcThread_.joinable()) {
    gcThread_.join();
  }

  // Make sure everything is shut down in reverse of construction order.
  // Cleanup is not necessary if overlay was not initialized
  if (!fsOverlay_.initialized()) {
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

  inodeMetadataTable_.reset();
  fsOverlay_.close(optNextInodeNumber);
}

folly::SemiFuture<Unit> Overlay::initialize() {
  // The initOverlay() call is potentially slow, so we want to avoid
  // performing it in the current thread and blocking returning to our caller.
  //
  // We already spawn a separate thread for garbage collection.  It's convenient
  // to simply use this existing thread to perform the initialization logic
  // before waiting for GC work to do.
  auto [initPromise, initFuture] = folly::makePromiseContract<Unit>();
  gcThread_ = std::thread([this, promise = std::move(initPromise)]() mutable {
    try {
      initOverlay();
    } catch (std::exception& ex) {
      XLOG(ERR) << "overlay initialization failed for "
                << fsOverlay_.getLocalDir() << ": " << ex.what();
      promise.setException(
          folly::exception_wrapper(std::current_exception(), ex));
      return;
    }
    promise.setValue();
    gcThread();
  });
  return std::move(initFuture);
}

void Overlay::initOverlay() {
  auto optNextInodeNumber = fsOverlay_.initOverlay(true);
  if (optNextInodeNumber) {
    nextInodeNumber_.store(
        optNextInodeNumber->get(), std::memory_order_relaxed);
  } else {
    // TODO: Run fsck code to detect and fix any fs corruption.
    XLOG(WARN) << "Overlay " << fsOverlay_.getLocalDir()
               << " was not shut down cleanly.  Will rescan.";
    nextInodeNumber_.store(
        fsOverlay_.scanForNextInodeNumber().get(), std::memory_order_relaxed);
  }

  // To support migrating from an older Overlay format, unconditionally create
  // tmp/.
  // TODO: It would be a bit expensive, but it might be worth checking
  // all of the numbered subdirectories here too.
  fsOverlay_.ensureTmpDirectoryIsCreated();

  // Open after infoFile_'s lock is acquired because the InodeTable acquires
  // its own lock, which should be released prior to infoFile_.
  inodeMetadataTable_ = InodeMetadataTable::open(
      (fsOverlay_.getLocalDir() + PathComponentPiece{FsOverlay::kMetadataFile})
          .c_str());
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
  DCHECK_NE(0, previous) << "allocateInodeNumber called before initialize";
  return InodeNumber{previous};
}

optional<DirContents> Overlay::loadOverlayDir(InodeNumber inodeNumber) {
  auto dirData = fsOverlay_.loadOverlayDir(inodeNumber);
  if (!dirData.has_value()) {
    return std::nullopt;
  }
  const auto& dir = dirData.value();

  bool shouldMigrateToNewFormat = false;

  DirContents result;
  for (auto& iter : dir.entries) {
    const auto& name = iter.first;
    const auto& value = iter.second;

    bool isMaterialized =
        !value.__isset.hash || value.hash_ref().value_unchecked().empty();
    InodeNumber ino;
    if (value.inodeNumber) {
      ino = InodeNumber::fromThrift(value.inodeNumber);
    } else {
      ino = allocateInodeNumber();
      shouldMigrateToNewFormat = true;
    }

    if (isMaterialized) {
      result.emplace(PathComponentPiece{name}, value.mode, ino);
    } else {
      auto hash = Hash{folly::ByteRange{
          folly::StringPiece{value.hash_ref().value_unchecked()}}};
      result.emplace(PathComponentPiece{name}, value.mode, ino, hash);
    }
  }

  if (shouldMigrateToNewFormat) {
    saveOverlayDir(inodeNumber, result);
  }

  return std::move(result);
}

void Overlay::saveOverlayDir(InodeNumber inodeNumber, const DirContents& dir) {
  auto nextInodeNumber = nextInodeNumber_.load(std::memory_order_relaxed);
  CHECK_LT(inodeNumber.get(), nextInodeNumber)
      << "saveOverlayDir called with unallocated inode number";

  // TODO: T20282158 clean up access of child inode information.
  //
  // Translate the data to the thrift equivalents
  overlay::OverlayDir odir;

  for (auto& entIter : dir) {
    const auto& entName = entIter.first;
    const auto& ent = entIter.second;

    CHECK_LT(ent.getInodeNumber().get(), nextInodeNumber)
        << "saveOverlayDir called with entry using unallocated inode number";

    overlay::OverlayEntry oent;
    // TODO: Eventually, we should merely serialize the child entry's dtype
    // into the Overlay. But, as of now, it's possible to create an inode under
    // a tree, serialize that tree into the overlay, then restart Eden. Since
    // writing mode bits into the InodeMetadataTable only occurs when the inode
    // is loaded, the initial mode bits must persist until the first load.
    oent.mode = ent.getInitialMode();
    oent.inodeNumber = ent.getInodeNumber().get();
    bool isMaterialized = ent.isMaterialized();
    if (!isMaterialized) {
      auto entHash = ent.getHash();
      auto bytes = entHash.getBytes();
      oent.set_hash(std::string{reinterpret_cast<const char*>(bytes.data()),
                                bytes.size()});
    }

    odir.entries.emplace(
        std::make_pair(entName.stringPiece().str(), std::move(oent)));
  }

  fsOverlay_.saveOverlayDir(inodeNumber, odir);
}

void Overlay::removeOverlayData(InodeNumber inodeNumber) {
  // TODO: batch request during GC
  getInodeMetadataTable()->freeInode(inodeNumber);
  fsOverlay_.removeOverlayFile(inodeNumber);
}

void Overlay::recursivelyRemoveOverlayData(InodeNumber inodeNumber) {
  auto dirData = fsOverlay_.loadOverlayDir(inodeNumber);

  // This inode's data must be removed from the overlay before
  // recursivelyRemoveOverlayData returns to avoid a race condition if
  // recursivelyRemoveOverlayData(I) is called immediately prior to
  // saveOverlayDir(I).  There's also no risk of violating our durability
  // guarantees if the process dies after this call but before the thread could
  // remove this data.
  removeOverlayData(inodeNumber);

  if (dirData) {
    gcQueue_.lock()->queue.emplace_back(std::move(*dirData));
    gcCondVar_.notify_one();
  }
}

folly::Future<folly::Unit> Overlay::flushPendingAsync() {
  folly::Promise<folly::Unit> promise;
  auto future = promise.getFuture();
  gcQueue_.lock()->queue.emplace_back(std::move(promise));
  gcCondVar_.notify_one();
  return future;
}

bool Overlay::hasOverlayData(InodeNumber inodeNumber) {
  return fsOverlay_.hasOverlayData(inodeNumber);
}

// Helper function to open,validate,
// get file pointer of an overlay file
folly::File Overlay::openFile(
    InodeNumber inodeNumber,
    folly::StringPiece headerId) {
  return fsOverlay_.openFile(inodeNumber, headerId);
}

folly::File Overlay::openFileNoVerify(InodeNumber inodeNumber) {
  return fsOverlay_.openFileNoVerify(inodeNumber);
}

folly::File Overlay::createOverlayFile(
    InodeNumber inodeNumber,
    folly::ByteRange contents) {
  CHECK_LT(inodeNumber.get(), nextInodeNumber_.load(std::memory_order_relaxed))
      << "createOverlayFile called with unallocated inode number";
  return fsOverlay_.createOverlayFile(inodeNumber, contents);
}

folly::File Overlay::createOverlayFile(
    InodeNumber inodeNumber,
    const folly::IOBuf& contents) {
  CHECK_LT(inodeNumber.get(), nextInodeNumber_.load(std::memory_order_relaxed))
      << "createOverlayFile called with unallocated inode number";
  return fsOverlay_.createOverlayFile(inodeNumber, contents);
}

InodeNumber Overlay::getMaxInodeNumber() {
  auto ino = nextInodeNumber_.load(std::memory_order_relaxed);
  CHECK_GT(ino, 1);
  return InodeNumber{ino - 1};
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
        gcCondVar_.wait(lock.getUniqueLock());
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
  if (request.flush) {
    request.flush->setValue();
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
    for (const auto& entry : dir.entries) {
      const auto& value = entry.second;
      if (!value.inodeNumber) {
        // Legacy-only.  All new Overlay trees have inode numbers for all
        // children.
        continue;
      }
      auto ino = InodeNumber::fromThrift(value.inodeNumber);

      if (S_ISDIR(value.mode)) {
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

  processDir(request.dir);

  while (!queue.empty()) {
    auto ino = queue.front();
    queue.pop();

    overlay::OverlayDir dir;
    try {
      auto dirData = fsOverlay_.loadOverlayDir(ino);
      if (!dirData.has_value()) {
        XLOG(DBG3) << "no dir data for inode " << ino;
        continue;
      } else {
        dir = std::move(*dirData);
      }
    } catch (const std::exception& e) {
      XLOG(ERR) << "While collecting, failed to load tree data for inode "
                << ino << ": " << e.what();
      continue;
    }

    safeRemoveOverlayData(ino);
    processDir(dir);
  }
}

} // namespace eden
} // namespace facebook
