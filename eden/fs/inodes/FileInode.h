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
#include <folly/Optional.h>
#include <folly/Synchronized.h>
#include "eden/fs/inodes/InodeBase.h"
#include "eden/fs/model/Tree.h"

namespace folly {
class File;
}

namespace facebook {
namespace eden {

class Blob;
class FileHandle;
class FileData;
class Hash;

class FileInode : public InodeBase {
 public:
  enum : int { WRONG_TYPE_ERRNO = EISDIR };

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
  folly::Future<fusell::Dispatcher::Attr> setattr(
      const struct stat& attr,
      int to_set) override;
  folly::Future<std::string> readlink();
  folly::Future<std::shared_ptr<fusell::FileHandle>> open(
      const struct fuse_file_info& fi);

  /** Specialized helper to finish a file creation operation.
   * Intended to be called immediately after invoking the constructor
   * that accepts a File object, this returns an opened FileHandle
   * for the file that was passed to the constructor. */
  std::shared_ptr<FileHandle> finishCreate();

  folly::Future<std::vector<std::string>> listxattr() override;
  folly::Future<std::string> getxattr(folly::StringPiece name) override;
  folly::Future<Hash> getSHA1(bool failIfSymlink = true);

  /// Ensure that underlying storage information is loaded
  std::shared_ptr<FileData> getOrLoadData();

  /// Compute the path to the overlay file for this item.
  AbsolutePath getLocalPath() const;

  /**
   * Check to see if the file has the same contents as the specified blob
   * and the same mode.
   *
   * This is more efficient than manually comparing the contents, as it can
   * perform a simple hash check if the file is not materialized.
   */
  bool isSameAs(const Blob& blob, mode_t mode);

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

 private:
  /**
   * The contents of a FileInode.
   *
   * This structure exists to allow the entire contents to be protected inside
   * folly::Synchronized.  This ensures proper synchronization when accessing
   * any member variables of FileInode.
   */
  struct State {
    State(FileInode* inode, mode_t mode, const folly::Optional<Hash>& hash);
    State(FileInode* inode, mode_t mode, folly::File&& hash, dev_t rdev = 0);

    std::shared_ptr<FileData> data;
    mode_t mode{0};
    dev_t rdev{0};
    folly::Optional<Hash> hash;
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
  std::shared_ptr<FileData> getOrLoadData(
      const folly::Synchronized<State>::LockedPtr& state);

  /**
   * Mark this FileInode materialized in its parent directory.
   */
  void materializeInParent();

  /// Called as part of shutting down an open handle.
  void fileHandleDidClose();

  folly::Synchronized<State> state_;

  friend class ::facebook::eden::FileHandle;
  friend class ::facebook::eden::FileData;
};
}
}
