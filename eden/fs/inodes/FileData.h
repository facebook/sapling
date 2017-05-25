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
#include <folly/Portability.h>
#include <folly/futures/Future.h>
#include <folly/io/IOBuf.h>
#include <mutex>
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/model/Tree.h"

namespace folly {
template <typename T>
class Optional;
}

namespace facebook {
namespace eden {
namespace fusell {
class BufVec;
}

class Blob;
class Hash;
class ObjectStore;
class Overlay;

/**
 * FileData stores information about a file contents.
 *
 * The data may be lazily loaded from the EdenMount's ObjectStore only when it
 * is needed.
 *
 * FileData objects are tracked via shared_ptr.  FileInode and FileHandle
 * objects maintain references to them.  FileData objects never outlive
 * the FileInode to which they belong.
 */
class FileData {
 public:
  /** Construct a FileData from an overlay entry */
  FileData(FileInode* inode, const folly::Optional<Hash>& hash);

  /** Construct a freshly created FileData from a pre-opened File object.
   * file must be moved in (it has no copy constructor) and must have
   * been created by a call to Overlay::createFile.  This constructor
   * is used in the TreeInode::create case and is required to implement
   * O_EXCL correctly. */
  FileData(FileInode* inode, folly::File&& file);

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
  fusell::BufVec read(size_t size, off_t off);
  size_t write(fusell::BufVec&& buf, off_t off);
  size_t write(folly::StringPiece data, off_t off);
  struct stat stat();
  void flush(uint64_t lock_owner);
  void fsync(bool datasync);

  /// Change attributes for this inode.
  // attr is a standard struct stat.  Only the members indicated
  // by to_set are valid.  Defined values for the to_set flags
  // are found in the fuse_lowlevel.h header file and have symbolic
  // names matching FUSE_SET_*.
  struct stat setAttr(const struct stat& attr, int to_set);

  /// Returns the sha1 hash of the content.
  Hash getSha1();

  /**
   * Read the entire file contents, and return them as a string.
   *
   * Note that this API generally should only be used for fairly small files.
   */
  std::string readAll();

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
   * Load the file data so it can be used for reading.
   *
   * If this file is materialized, this opens it's file in the overlay.
   * If the file is not materialized, this loads the Blob data from the
   * ObjectStore.
   */
  FOLLY_NODISCARD folly::Future<folly::Unit> ensureDataLoaded();

 private:
  ObjectStore* getObjectStore() const;

  /// Recompute the SHA1 content hash of the open file_.
  Hash recomputeAndStoreSha1(
      const folly::Synchronized<FileInode::State>::LockedPtr& state);
  void storeSha1(
      const folly::Synchronized<FileInode::State>::LockedPtr& state,
      Hash sha1);

  /**
   * The FileInode that this FileData object belongs to.
   *
   * This pointer never changes once a FileData object is constructed.  A
   * FileData always belongs to the same FileInode.  Therefore it is safe to
   * access this pointer without locking.
   */
  FileInode* const inode_{nullptr};

  /// if backed by tree, the data from the tree, else nullptr.
  std::unique_ptr<Blob> blob_;

  /// if backed by an overlay file, the open file descriptor
  folly::File file_;

  /// if backed by an overlay file, whether the sha1 xattr is valid
  bool sha1Valid_{false};
};
}
}
