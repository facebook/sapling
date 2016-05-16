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
class LocalStore;
class Overlay;

class FileData {
 public:
  FileData(
      std::mutex& mutex,
      std::shared_ptr<LocalStore> store,
      std::shared_ptr<Overlay> overlay,
      const TreeEntry* entry);

  fusell::BufVec read(size_t size, off_t off);

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
  std::shared_ptr<LocalStore> store_;
  std::shared_ptr<Overlay> overlay_;
  const TreeEntry* entry_;

  /// if backed by tree, the data from the tree, else nullptr.
  std::unique_ptr<Blob> blob_;

  /// if backed by an overlay file, the open file descriptor
  folly::File file_;
};
}
}
