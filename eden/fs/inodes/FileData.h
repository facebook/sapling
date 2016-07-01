/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once
#include <folly/File.h>
#include <mutex>
#include "eden/fs/model/Tree.h"
#include "eden/fuse/BufVec.h"

namespace facebook {
namespace eden {

class Blob;
class EdenMount;
class Hash;

/**
 * FileData stores information about a file contents.
 *
 * The data may be lazily loaded from the EdenMount's ObjectStore only when it
 * is needed.
 *
 * FileData objects are tracked via shared_ptr.  TreeEntryFileInode and
 * TreeEntryFileHandle objects maintain references to them.  FileData objects
 * should not outlive the EdenMount to which they belong.
 */
class FileData {
 public:
  FileData(std::mutex& mutex, EdenMount* mount, const TreeEntry* entry);

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
  /// Returns the sha1 hash of the content, for existing lock holders.
  Hash getSha1Locked(const std::unique_lock<std::mutex>&);

  /// Materialize the file data.
  // open_flags has the same meaning as the flags parameter to
  // open(2).  Materialization depends on the write mode specified
  // in those flags; if we are writing to the file then we need to
  // copy it locally to the overlay.  If we are truncating we just
  // need to create an empty file in the overlay.  Otherwise we
  // need to go out to the LocalStore to obtain the backing data.
  void materialize(int open_flags, RelativePathPiece path);

 private:
  // Reference to the mutex in the associated inode.
  // It must be held by readers and writers before interpreting the filedata,
  // as any actor may cause materialization or truncation of the data.
  // Recommended practice in the implementation of methods on this class is to
  // hold a unique_lock as a guard for the duration of the method.
  std::mutex& mutex_;

  /**
   * The EdenMount that this FileData object belongs to.
   *
   * This pointer never changes once a FileData object is constructed.  A
   * FileData always belongs to the same EdenMount.  Therefore it is safe to
   * access this pointer without locking.
   */
  EdenMount* const mount_{nullptr};

  /// The TreeEntry for this file.
  const TreeEntry* entry_;

  /// if backed by tree, the data from the tree, else nullptr.
  std::unique_ptr<Blob> blob_;

  /// if backed by an overlay file, the open file descriptor
  folly::File file_;

  /// if backed by an overlay file, whether the sha1 xattr is valid
  bool sha1Valid_{false};

  /// Recompute the SHA1 content hash of the open file_.
  // The mutex must be owned by the caller.
  Hash recomputeAndStoreSha1();
};
}
}
