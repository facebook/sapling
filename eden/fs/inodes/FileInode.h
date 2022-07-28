/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Synchronized.h>
#include <folly/futures/SharedPromise.h>
#include <chrono>
#include <optional>
#include "eden/fs/inodes/CacheHint.h"
#include "eden/fs/inodes/InodeBase.h"
#include "eden/fs/model/BlobMetadata.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/BlobCache.h"
#include "eden/fs/store/IObjectStore.h"
#include "eden/fs/store/ImportPriority.h"
#include "eden/fs/utils/BufVec.h"
#include "eden/fs/utils/CoverageSet.h"

namespace folly {
class File;
}

namespace facebook::eden {

class Blob;
class ObjectFetchContext;
class ObjectStore;
class OverlayFileAccess;

/**
 * The contents of a FileInode.
 *
 * This structure exists to allow the entire contents to be protected inside
 * folly::Synchronized.  This ensures proper synchronization when accessing
 * any member variables of FileInode.
 *
 * A FileInode can be in one of three states:
 *   - not loading: the blob may be in cache, but it is not currently being
 *                  loaded
 *   - loading: fetching data from backing store, but it's not available yet
 *   - materialized: contents are written into overlay
 *
 * Valid state transitions:
 *   - not loading -> loading
 *   - not loading -> materialized (O_TRUNC)
 *   - loading -> not loaded (blob available during transition)
 *   - loading -> materialized (O_TRUNC or not)
 *   - loading -> not loading -> materialized
 */
struct FileInodeState {
  enum Tag : uint8_t {
    BLOB_NOT_LOADING,
    BLOB_LOADING,
    MATERIALIZED_IN_OVERLAY,
  };

  explicit FileInodeState(const std::optional<ObjectId>& hash);
  explicit FileInodeState();
  ~FileInodeState();

  /**
   * In lieu of std::variant, enforce the state machine invariants.
   * Called after construction and each time we unlock the state.
   */
  void checkInvariants();

  /**
   * Returns true if the file is materialized in the overlay.
   */
  bool isMaterialized() const {
    return tag == MATERIALIZED_IN_OVERLAY;
  }

  Tag tag;

  struct NonMaterializedState {
    ObjectId hash;

    /**
     * Cached size to speedup FileInode::stat calls. The max uint64_t value is
     * used to represent a non-cached size, this is used instead of a
     * std::optional to save 8 bytes.
     */
    static constexpr uint64_t kUnknownSize =
        std::numeric_limits<uint64_t>::max();
    uint64_t size{kUnknownSize};

    explicit NonMaterializedState(const ObjectId& hash) : hash(hash) {}
  };

  /**
   * Set only in 'not loading' and 'loading' states. std::nullopt otherwise.
   */
  std::optional<NonMaterializedState> nonMaterializedState;

  /**
   * Set if 'loading'. Unset when load completes.
   *
   * It's possible for this future to complete with a null blob - that happens
   * if a truncate operation occurs during load. In that case, the future is
   * completed and the inode transitions to the materialized state without
   * a blob. Callbacks on this future must handle that case.
   */
  std::unique_ptr<folly::SharedPromise<std::shared_ptr<const Blob>>>
      blobLoadingPromise;

  /**
   * If the blob has ever been loaded from cache, this handle represents this
   * inode's interest in it. By explicitly resetting the interest handle, the
   * inode indicates to the cache that the blob can be released.
   *
   * This also indicates to the cache that the blob is no longer needed in
   * memory when the FileInode is deallocated.
   *
   * Before attempting to reload the blob, check if the interestHandle has it
   * first.
   */
  BlobInterestHandle interestHandle;

#ifndef _WIN32
  /**
   * Records the ranges that have been read() when not materialized.
   */
  CoverageSet readByteRanges;
#endif
};

class FileInode final : public InodeBaseMetadata<FileInodeState> {
 public:
  using Base = InodeBaseMetadata<FileInodeState>;

  enum : int { WRONG_TYPE_ERRNO = EISDIR };

  /**
   * If hash is none, this opens the file in the overlay and leaves the inode
   * in MATERIALIZED_IN_OVERLAY state.  If hash is set, the inode is in
   * NOT_LOADED state.
   */
  FileInode(
      InodeNumber ino,
      TreeInodePtr parentInode,
      PathComponentPiece name,
      mode_t initialMode,
      const std::optional<InodeTimestamps>& initialTimestamps,
      const std::optional<ObjectId>& hash);

  /**
   * Construct an inode using a freshly created overlay file.
   */
  FileInode(
      InodeNumber ino,
      TreeInodePtr parentInode,
      PathComponentPiece name,
      mode_t initialMode,
      const InodeTimestamps& initialTimestamps);

#ifndef _WIN32
  ImmediateFuture<struct stat> setattr(
      const DesiredMetadata& desired,
      ObjectFetchContext& fetchContext) override;

  /// Throws InodeError EINVAL if inode is not a symbolic node.
  ImmediateFuture<std::string> readlink(
      ObjectFetchContext& fetchContext,
      CacheHint cacheHint = CacheHint::LikelyNeededAgain);

  ImmediateFuture<std::string> getxattr(
      folly::StringPiece name,
      ObjectFetchContext& context) override;
  ImmediateFuture<std::vector<std::string>> listxattr() override;
#endif

  ImmediateFuture<Hash20> getSha1(ObjectFetchContext& fetchContext);

  ImmediateFuture<BlobMetadata> getBlobMetadata(
      ObjectFetchContext& fetchContext);

  /**
   * Check to see if the file has the same contents as the specified blob
   * and the same tree entry type.
   *
   * This is more efficient than manually comparing the contents, as it may be
   * able to perform a simple hash check if the file is not materialized.
   */
  ImmediateFuture<bool> isSameAs(
      const Blob& blob,
      TreeEntryType entryType,
      ObjectFetchContext& fetchContext);
  ImmediateFuture<bool> isSameAs(
      const ObjectId& blobID,
      const Hash20& blobSha1,
      TreeEntryType entryType,
      ObjectFetchContext& fetchContext);
  ImmediateFuture<bool> isSameAs(
      const ObjectId& blobID,
      TreeEntryType entryType,
      ObjectFetchContext& fetchContext);

  /**
   * Get the file mode_t value.
   */
  mode_t getMode() const;

#ifndef _WIN32
  /**
   * Get the file dev_t value.
   */
  dev_t getRdev() const;

  /**
   * Get the permissions bits from the file mode.
   *
   * This returns the mode with the file type bits masked out.
   */
  mode_t getPermissions() const;

  /**
   * Returns a copy of this inode's metadata.
   */
  InodeMetadata getMetadata() const override;
#endif // !_WIN32

  void forceMetadataUpdate() override;

  /**
   * If this file is backed by a source control Blob, return the hash of the
   * Blob, or return std::nullopt if this file is materialized in the overlay.
   *
   * Beware that the file's materialization state may have changed by the time
   * you use the return value of this method.  This method is primarily
   * intended for use in tests and debugging functions.  Its return value
   * generally cannot be trusted in situations where there may be concurrent
   * modifications by other threads.
   */
  std::optional<ObjectId> getBlobHash() const;

  /**
   * Read the entire file contents, and return them as a string.
   *
   * Note that this API generally should only be used for fairly small files.
   */
  FOLLY_NODISCARD ImmediateFuture<std::string> readAll(
      ObjectFetchContext& fetchContext,
      CacheHint cacheHint = CacheHint::LikelyNeededAgain);

#ifdef _WIN32
  // This function will update the FileInode's state as materialized. This is a
  // Windows only function. On POSIX systems the write() functions mark a file
  // as Materialized.
  void materialize();
#else
  /**
   * Read up to size bytes from the file at the specified offset.
   *
   * Returns a tuple of a BufVec containing the data and a boolean indicating
   * if the end-of-file was reached.  This may return fewer bytes than
   * requested.  If the specified offset is at or past the end of the buffer an
   * empty IOBuf will be returned.  Otherwise between 1 and size bytes will be
   * returned.  If fewer than size bytes are returned this does *not* guarantee
   * that the end of the file was reached, the boolean should be checked for
   * this.
   *
   * May throw exceptions on error.
   */
  ImmediateFuture<std::tuple<BufVec, bool>>
  read(size_t size, off_t off, ObjectFetchContext& context);

  ImmediateFuture<size_t>
  write(BufVec&& buf, off_t off, ObjectFetchContext& fetchContext);
  ImmediateFuture<size_t>
  write(folly::StringPiece data, off_t off, ObjectFetchContext& fetchContext);

  void fsync(bool datasync);

  FOLLY_NODISCARD ImmediateFuture<folly::Unit>
  fallocate(uint64_t offset, uint64_t length, ObjectFetchContext& fetchContext);

  ImmediateFuture<folly::Unit> ensureMaterialized(
      ObjectFetchContext& fetchContext,
      bool followSymlink) override;

#endif // !_WIN32

  ImmediateFuture<struct stat> stat(ObjectFetchContext& context) override;

 private:
  using State = FileInodeState;
  class LockedState;

  /**
   * Run a function with the FileInode data loaded.
   *
   * fn(state, blob) will be invoked when state->tag is either NOT_LOADING or
   * MATERIALIZED_IN_OVERLAY. If state->tag is MATERIALIZED_IN_OVERLAY,
   * state->file will be available. If state->tag is NOT_LOADING, then the
   * second argument will be a non-null std::shared_ptr<const Blob>.
   *
   * The blob parameter is used when recursing.
   *
   * Returns an ImmediateFuture with the result of fn(state_.wlock(), blob)
   */
  template <typename Fn>
  ImmediateFuture<
      std::invoke_result_t<Fn, LockedState&&, std::shared_ptr<const Blob>>>
  runWhileDataLoaded(
      LockedState state,
      BlobCache::Interest interest,
      ObjectFetchContext& fetchContext,
      std::shared_ptr<const Blob> blob,
      Fn&& fn);

#ifndef _WIN32
  /**
   * Run a function with the FileInode materialized.
   *
   * fn(state) will be invoked when state->tag is MATERIALIZED_IN_OVERLAY.
   *
   * Returns an ImmediateFuture with the result of fn(state_.wlock())
   */
  template <typename Fn>
  ImmediateFuture<std::invoke_result_t<Fn, LockedState&&>> runWhileMaterialized(
      LockedState state,
      std::shared_ptr<const Blob> blob,
      Fn&& fn,
      ObjectFetchContext& fetchContext,
      std::optional<std::chrono::system_clock::time_point> startTime =
          std::nullopt);

  /**
   * Truncate the file and then call a function.
   *
   * This immediately truncates the file, and never has to wait for data to
   * load from the ObjectStore.
   *
   * fn(state) will be invoked with state->tag set to MATERIALIZED_IN_OVERLAY.
   *
   * Returns the result of fn(state_.wlock())
   */
  template <typename Fn>
  typename std::invoke_result_t<Fn, LockedState&&> truncateAndRun(
      LockedState state,
      Fn&& fn);

#endif // !_WIN32

  /**
   * Start loading the file data.
   *
   * state->tag must be NOT_LOADED when this is called.
   *
   * This should normally only be invoked by runWhileDataLoaded() or
   * runWhileMaterialized().  Most other callers should use
   * runWhileDataLoaded() or runWhileMaterialized() instead.
   */
  FOLLY_NODISCARD ImmediateFuture<std::shared_ptr<const Blob>> startLoadingData(
      LockedState state,
      BlobCache::Interest interest,
      ObjectFetchContext& fetchContext);

#ifndef _WIN32
  /**
   * Materialize the file as an empty file in the overlay.
   *
   * state->tag must not already be MATERIALIZED_IN_OVERLAY when this is called.
   *
   * After this function returns the caller must call materializeInParent()
   * after releasing the state lock.  If the state was previously BLOB_LOADING
   * the caller must also fulfill the blobLoadingPromise.
   *
   * This should normally only be invoked by truncateAndRun().  Most callers
   * should use truncateAndRun() instead of calling this function directly.
   */
  void materializeAndTruncate(LockedState& state);

  /**
   * Replace this file's contents in the overlay with an empty file.
   *
   * state->tag must be MATERIALIZED_IN_OVERLAY when this is called.
   *
   * This should normally only be invoked by truncateAndRun().  Most callers
   * should use truncateAndRun() instead of calling this function directly.
   */
  void truncateInOverlay(LockedState& state);

#endif // !_WIN32

  /**
   * Transition from NOT_LOADING to MATERIALIZED_IN_OVERLAY by copying the
   * blob into the overlay.
   */
  void materializeNow(
      LockedState& state,
      std::shared_ptr<const Blob> blob,
      ObjectFetchContext& fetchContext);

  /**
   * Get a FileInodePtr to ourself.
   *
   * This uses FileInodePtr::newPtrFromExisting() internally.
   *
   * This should only be called in contexts where we know an external caller
   * already has an existing reference to us.  (Which is most places--a caller
   * has to have a reference to us in order to call any of our APIs.)
   */
  FileInodePtr inodePtrFromThis() {
    return FileInodePtr::newPtrFromExisting(this);
  }

  /**
   * Mark this FileInode materialized in its parent directory.
   *
   * The state_ lock must not be held when calling this method.
   */
  void materializeInParent();

  /**
   * Helper function for isSameAs().
   *
   * This does the initial portion of the check which never requires an
   * ImmediateFuture.  Returns optional<bool> if the check completes
   * immediately, or std::nullopt if the contents need to be checked against
   * sha1 of file contents.
   */
  std::optional<bool> isSameAsFast(
      const ObjectId& blobID,
      TreeEntryType entryType);

  /**
   * Helper function for isSameAs().
   *
   * This does the second portion of the check which requires an
   * ImmediateFuture. Returns ImmediateFuture<bool> that checks the inodes
   * Sha1 against the given Sha1.
   */
  ImmediateFuture<bool> isSameAsSlow(
      const Hash20& expectedBlobSha1,
      ObjectFetchContext& fetchContext);

#ifndef _WIN32
  /**
   * Returns the OverlayFileAccess used to mediate access to an overlay file.
   *
   * An unused LockedState& is passed in to help avoid unsynchronized access.
   * (Don't use the returned OverlayFileAccess outside of the lock).
   */
  OverlayFileAccess* getOverlayFileAccess(LockedState&) const;

  size_t writeImpl(
      LockedState& state,
      const struct iovec* iov,
      size_t numIovecs,
      off_t off);
#endif // !_WIN32

  /**
   * Update the st_blocks field in a stat structure based on the st_size value.
   */
  static void updateBlockCount(struct stat& st);

#ifdef _WIN32
  /**
   * The getMaterializedFilePath() will return the Absolute path to the file in
   * the ProjectedFS cache.
   */
  AbsolutePath getMaterializedFilePath();
#endif // _WIN32

  /**
   * Log accesses via the ServerState's HiveLogger.
   */
  void logAccess(ObjectFetchContext& fetchContext);

  folly::Synchronized<State> state_;

  // So it can call inodePtrFromThis() for better error messages.
  friend class ::facebook::eden::OverlayFileAccess;
};
} // namespace facebook::eden
