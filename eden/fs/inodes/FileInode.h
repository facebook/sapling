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
#include <folly/Optional.h>
#include <folly/Synchronized.h>
#include <folly/futures/SharedPromise.h>
#include <chrono>
#include "eden/fs/inodes/InodeBase.h"
#include "eden/fs/model/Tree.h"

namespace folly {
class File;
}

namespace facebook {
namespace eden {

namespace fusell {
class BufVec;
}

class Blob;
class FileHandle;
class Hash;
class ObjectStore;

class FileInode : public InodeBase {
 public:
  enum : int { WRONG_TYPE_ERRNO = EISDIR };

  /**
   * The FUSE create request wants both the inode and a file handle.  This
   * constructor simultaneously allocates a FileInode given the File and
   * returns a new FileHandle to it.
   */
  static std::tuple<FileInodePtr, std::shared_ptr<FileHandle>> create(
      fuse_ino_t ino,
      TreeInodePtr parentInode,
      PathComponentPiece name,
      mode_t mode,
      folly::File&& file,
      dev_t rdev = 0);

  /** Construct an inode using an overlay entry */
  FileInode(
      fuse_ino_t ino,
      TreeInodePtr parentInode,
      PathComponentPiece name,
      mode_t mode,
      const folly::Optional<Hash>& hash);

  /** Construct an inode using a freshly created overlay file.
   * file must be moved in and must have been created by a call to
   * Overlay::openFile.  This constructor is used in the TreeInode::create
   * case and is required to implement O_EXCL correctly. */
  FileInode(
      fuse_ino_t ino,
      TreeInodePtr parentInode,
      PathComponentPiece name,
      mode_t mode,
      folly::File&& file,
      dev_t rdev = 0);

  folly::Future<fusell::Dispatcher::Attr> getattr() override;

  /// Throws InodeError EINVAL if inode is not a symbolic node.
  folly::Future<std::string> readlink();

  folly::Future<std::shared_ptr<fusell::FileHandle>> open(
      const struct fuse_file_info& fi);

  folly::Future<std::vector<std::string>> listxattr() override;
  folly::Future<std::string> getxattr(folly::StringPiece name) override;

  folly::Future<folly::Unit> prefetch() override;

  /**
   * Updates in-memory timestamps in FileInode and TreeInode to the overlay
   * file.
   */
  void updateOverlayHeader() const override;
  folly::Future<Hash> getSha1(bool failIfSymlink = true);

  /**
   * Compute the path to the overlay file for this item.
   */
  AbsolutePath getLocalPath() const;

  /**
   * Check to see if the file has the same contents as the specified blob
   * and the same mode.
   *
   * This is more efficient than manually comparing the contents, as it can
   * perform a simple hash check if the file is not materialized.
   */
  bool isSameAs(const Blob& blob, mode_t mode);
  folly::Future<bool> isSameAs(const Hash& blobID, mode_t mode);

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
   * If this file is backed by a source control Blob, return the hash of the
   * Blob, or return folly::none if this file is materialized in the overlay.
   *
   * Beware that the file's materialization state may have changed by the time
   * you use the return value of this method.  This method is primarily
   * intended for use in tests and debugging functions.  Its return value
   * generally cannot be trusted in situations where there may be concurrent
   * modifications by other threads.
   */
  folly::Optional<Hash> getBlobHash() const;

  /**
   * Read the entire file contents, and return them as a string.
   *
   * Note that this API generally should only be used for fairly small files.
   */
  FOLLY_NODISCARD folly::Future<std::string> readAll();

  /**
   * Read up to size bytes from the file at the specified offset.
   *
   * Returns an IOBuf containing the data.  This may return fewer bytes than
   * requested.  If the specified offset is at or past the end of the buffer an
   * empty IOBuf will be returned.  Otherwise between 1 and size bytes will be
   * returned.  If fewer than size bytes are returned this does *not* guarantee
   * that the end of the file was reached.
   *
   * May throw exceptions on error.
   */
  std::unique_ptr<folly::IOBuf> readIntoBuffer(size_t size, off_t off);

  size_t write(folly::StringPiece data, off_t off);

  /**
   * Get the timestamps of the inode.
   */
  InodeTimestamps getTimestamps() const;

 private:
  /**
   * Load the file data so it can be used for reading.
   *
   * If this file is materialized, this opens its file in the overlay.
   * If the file is not materialized, this loads the Blob data from the
   * ObjectStore.
   */
  FOLLY_NODISCARD folly::Future<folly::Unit> ensureDataLoaded();

  /**
   * Materialize the file data.
   * openFlags has the same meaning as the flags parameter to
   * open(2).  Materialization depends on the write mode specified
   * in those flags; if we are writing to the file then we need to
   * copy it locally to the overlay.  If we are truncating we just
   * need to create an empty file in the overlay.  Otherwise we
   * need to go out to the LocalStore to obtain the backing data.
   */
  FOLLY_NODISCARD folly::Future<folly::Unit> materializeForWrite(int openFlags);

  /**
   * Ensures the inode transitions to or stays in the 'materialized' state,
   * and truncates the file to zero bytes.
   */
  void materializeAndTruncate();

  /**
   * The contents of a FileInode.
   *
   * This structure exists to allow the entire contents to be protected inside
   * folly::Synchronized.  This ensures proper synchronization when accessing
   * any member variables of FileInode.
   *
   * A FileInode can be in one of three states:
   *   - not loaded
   *   - loading: fetching data from backing store, but it's not available yet
   *   - loaded: contents has been imported from mercurial and is accessible
   *   - materialized: contents are written into overlay and file handle is open
   *
   * Valid state transitions:
   *   - not loaded -> loading
   *   - not loaded -> materialized (O_TRUNC)
   *   - loading -> loaded
   *   - loading -> materialized (O_TRUNC)
   *   - loaded -> materialized
   */
  struct State {
    State(
        FileInode* inode,
        mode_t mode,
        const folly::Optional<Hash>& hash,
        const timespec& lastCheckoutTime);
    State(
        FileInode* inode,
        mode_t mode,
        folly::File&& hash,
        const timespec& lastCheckoutTime,
        dev_t rdev = 0);
    ~State();

    /**
     * In lieu of std::variant, enforce the state machine invariants.
     * Call after construction and on every modification.
     */
    void checkInvariants();

    mode_t mode{0};

    // TODO: Since rdev is immutable, move it out of the locked state.
    const dev_t rdev{0};

    /**
     * Set only in 'not loaded', 'loading', and 'loaded' states, none otherwise.
     * TODO: Perhaps we ought to simply leave this defined...
     */
    folly::Optional<Hash> hash;

    /**
     * Set if 'loading'.
     */
    folly::Optional<folly::SharedPromise<folly::Unit>> blobLoadingPromise;

    /**
     * Set if 'loaded', references immutable data from the backing store.
     */
    std::shared_ptr<const Blob> blob;

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
     * Timestamps for FileInode.
     */
    InodeTimestamps timeStamps;
  };

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
   * Returns Optional<bool> if the check completes immediately, or
   * folly::none if the contents need to be checked against sha1 of file
   * contents.
   */
  folly::Optional<bool> isSameAsFast(const Hash& blobID, mode_t mode);

  /**
   * Recompute the SHA1 content hash of the open file.
   */
  Hash recomputeAndStoreSha1(
      const folly::Synchronized<FileInode::State>::LockedPtr& state);

  ObjectStore* getObjectStore() const;
  static void storeSha1(
      const folly::Synchronized<FileInode::State>::LockedPtr& state,
      Hash sha1);

  fusell::BufVec read(size_t size, off_t off);
  size_t write(fusell::BufVec&& buf, off_t off);

  folly::Future<struct stat> stat();
  void flush(uint64_t lock_owner);
  void fsync(bool datasync);

  /**
   * Helper function used in setattr to perform FileInode specific operations
   * during setattr.
   */
  folly::Future<fusell::Dispatcher::Attr> setInodeAttr(
      const struct stat& attr,
      int to_set) override;

  folly::Synchronized<State> state_;

  friend class ::facebook::eden::FileHandle;
};
} // namespace eden
} // namespace facebook
