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
#include <folly/Synchronized.h>
#include <folly/futures/Future.h>
#include <folly/futures/SharedPromise.h>
#include <chrono>
#include <optional>
#include "eden/fs/inodes/CacheHint.h"
#include "eden/fs/inodes/InodeBase.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/BlobCache.h"

namespace folly {
class File;
}

namespace facebook {
namespace eden {

class Blob;
class BufVec;
class EdenFileHandle;
class Hash;
class ObjectStore;

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
 *   - materialized: contents are written into overlay and file handle is open
 *
 * Valid state transitions:
 *   - not loading -> loading
 *   - not loading -> materialized (O_TRUNC)
 *   - loading -> not loaded (blob available during transition)
 *   - loading -> materialized (O_TRUNC or not)
 *   - loading -> not loading -> materialized
 */
struct FileInodeState {
  using FileHandlePtr = std::shared_ptr<EdenFileHandle>;

  enum Tag : uint8_t {
    BLOB_NOT_LOADING,
    BLOB_LOADING,
    MATERIALIZED_IN_OVERLAY,
  };

  explicit FileInodeState(const std::optional<Hash>& hash);
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

  /**
   * Returns true if we're maintaining an open file.
   */
  bool isFileOpen() const {
    return bool(file);
  }

  /**
   * Decrement the openCount, closing any open overlay file handles if the open
   * count is now zero.
   */
  void decOpenCount();

  /**
   * Increment the open count.
   */
  void incOpenCount();

  Tag tag;

  /**
   * Set only in 'not loading' and 'loading' states. std::nullopt otherwise.
   */
  std::optional<Hash> hash;

  /**
   * Set if 'loading'. Unset when load completes.
   *
   * It's possible for this future to complete with a null blob - that happens
   * if a truncate operation occurs during load. In that case, the future is
   * completed and the inode transitions to the materialized state without
   * a blob. Callbacks on this future must handle that case.
   */
  std::optional<folly::SharedPromise<std::shared_ptr<const Blob>>>
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

  /**
   * If backed by an overlay file, whether the sha1 xattr is valid
   */
  bool sha1Valid{false};

  /**
   * Set if 'materialized', holds the open file descriptor backed by an
   * overlay file.
   */
  folly::File file;

  /**
   * Number of open file handles referencing us.
   */
  size_t openCount{0};
};

class FileInode final : public InodeBaseMetadata<FileInodeState> {
 public:
  using Base = InodeBaseMetadata<FileInodeState>;
  using FileHandlePtr = std::shared_ptr<EdenFileHandle>;

  enum : int { WRONG_TYPE_ERRNO = EISDIR };

  /**
   * The FUSE create request wants both the inode and a file handle.  This
   * constructor simultaneously allocates a FileInode given the File and
   * returns a new EdenFileHandle to it.
   *
   * The given timestamp populates all three timestamp fields.
   */
  static std::tuple<FileInodePtr, FileHandlePtr> create(
      InodeNumber ino,
      TreeInodePtr parentInode,
      PathComponentPiece name,
      mode_t initialMode,
      const InodeTimestamps& initialTimestamps,
      folly::File&& file);

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
      const std::optional<Hash>& hash);

  /**
   * Construct an inode using a freshly created overlay file.
   * file must be moved in and must have been created by a call to
   * Overlay::openFile.
   */
  FileInode(
      InodeNumber ino,
      TreeInodePtr parentInode,
      PathComponentPiece name,
      mode_t initialMode,
      const InodeTimestamps& initialTimestamps);

  folly::Future<Dispatcher::Attr> getattr() override;
  folly::Future<Dispatcher::Attr> setattr(const fuse_setattr_in& attr) override;

  /// Throws InodeError EINVAL if inode is not a symbolic node.
  folly::Future<std::string> readlink(
      CacheHint cacheHint = CacheHint::LikelyNeededAgain);

  folly::Future<std::shared_ptr<FileHandle>> open(int flags);

  folly::Future<std::string> getxattr(folly::StringPiece name) override;
  folly::Future<std::vector<std::string>> listxattr() override;

  folly::Future<folly::Unit> prefetch() override;

  folly::Future<Hash> getSha1();

  /**
   * Check to see if the file has the same contents as the specified blob
   * and the same tree entry type.
   *
   * This is more efficient than manually comparing the contents, as it may be
   * able to perform a simple hash check if the file is not materialized.
   */
  folly::Future<bool> isSameAs(const Blob& blob, TreeEntryType entryType);
  folly::Future<bool> isSameAs(const Hash& blobID, TreeEntryType entryType);

  /**
   * Get the file mode_t value.
   */
  mode_t getMode() const;

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
  std::optional<Hash> getBlobHash() const;

  /**
   * Read the entire file contents, and return them as a string.
   *
   * Note that this API generally should only be used for fairly small files.
   */
  FOLLY_NODISCARD folly::Future<std::string> readAll(
      CacheHint cacheHint = CacheHint::LikelyNeededAgain);

  /**
   * Read up to size bytes from the file at the specified offset.
   *
   * Returns a BufVec containing the data.  This may return fewer bytes than
   * requested.  If the specified offset is at or past the end of the buffer an
   * empty IOBuf will be returned.  Otherwise between 1 and size bytes will be
   * returned.  If fewer than size bytes are returned this does *not* guarantee
   * that the end of the file was reached.
   *
   * May throw exceptions on error.
   *
   * Precondition: openCount > 0.  This is held because read is only called by
   * FileInode or FileHandle.
   */
  folly::Future<BufVec> read(size_t size, off_t off);

  folly::Future<size_t> write(BufVec&& buf, off_t off);
  folly::Future<size_t> write(folly::StringPiece data, off_t off);

  void fsync(bool datasync);

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
   * Returns a Future with the result of fn(state_.wlock(), blob)
   */
  template <typename ReturnType, typename Fn>
  ReturnType runWhileDataLoaded(
      LockedState state,
      BlobCache::Interest interest,
      std::shared_ptr<const Blob> blob,
      Fn&& fn);

  /**
   * Run a function with the FileInode materialized.
   *
   * fn(state) will be invoked when state->tag is MATERIALIZED_IN_OVERLAY.
   * state->file will also be open.
   *
   * Returns a Future with the result of fn(state_.wlock())
   */
  template <typename Fn>
  typename folly::futures::detail::callableResult<LockedState, Fn>::Return
  runWhileMaterialized(
      LockedState state,
      std::shared_ptr<const Blob> blob,
      Fn&& fn);

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
  typename std::result_of<Fn(LockedState&&)>::type truncateAndRun(
      LockedState state,
      Fn&& fn);

  /**
   * Start loading the file data.
   *
   * state->tag must be NOT_LOADED when this is called.
   *
   * This should normally only be invoked by runWhileDataLoaded() or
   * runWhileMaterialized().  Most other callers should use
   * runWhileDataLoaded() or runWhileMaterialized() instead.
   */
  FOLLY_NODISCARD folly::Future<std::shared_ptr<const Blob>> startLoadingData(
      LockedState state,
      BlobCache::Interest interest);

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
   *
   * The input LockedState object will be updated to hold an open refcount,
   * and state->file will be valid when this function returns.
   */
  void materializeAndTruncate(LockedState& state);

  /**
   * Replace this file's contents in the overlay with an empty file.
   *
   * state->tag must be MATERIALIZED_IN_OVERLAY when this is called.
   *
   * This should normally only be invoked by truncateAndRun().  Most callers
   * should use truncateAndRun() instead of calling this function directly.
   *
   * The input LockedState object will be updated to hold an open refcount,
   * and state->file will be valid when this function returns.
   */
  void truncateInOverlay(LockedState& state);

  /**
   * Transition from NOT_LOADING to MATERIALIZED_IN_OVERLAY by copying the
   * blob into the overlay.
   *
   * The input LockedState object will be updated to hold an open refcount,
   * and state->file will be valid when this function returns.
   */
  void materializeNow(LockedState& state, std::shared_ptr<const Blob> blob);

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
   * Called as part of shutting down an open handle.
   */
  void fileHandleDidClose();

  /**
   * Helper function for isSameAs().
   *
   * This does the initial portion of the check which never requires a Future.
   * Returns optional<bool> if the check completes immediately, or
   * std::nullopt if the contents need to be checked against sha1 of file
   * contents.
   */
  std::optional<bool> isSameAsFast(const Hash& blobID, TreeEntryType entryType);

  /**
   * Recompute the SHA1 content hash of the open file.
   *
   * state->tag must be MATERIALIZED_IN_OVERLAY, and state.ensureFileOpen()
   * must have already been called.
   */
  Hash recomputeAndStoreSha1(const LockedState& state);

  /**
   * Store the SHA1 content hash on an overlay file.
   *
   * state->tag must be MATERIALIZED_IN_OVERLAY, and state.ensureFileOpen()
   * must have already been called.
   */
  static void storeSha1(const LockedState& state, Hash sha1);

  /**
   * Get the ObjectStore used by this FileInode to load non-materialized data.
   */
  ObjectStore* getObjectStore() const;

  size_t writeImpl(
      LockedState& state,
      const struct iovec* iov,
      size_t numIovecs,
      off_t off);

  folly::Future<struct stat> stat();

  /**
   * Update the st_blocks field in a stat structure based on the st_size value.
   */
  static void updateBlockCount(struct stat& st);

  folly::Synchronized<State> state_;

  friend class ::facebook::eden::EdenFileHandle;
};
} // namespace eden
} // namespace facebook
