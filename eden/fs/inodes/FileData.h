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
#include <mutex>
#include "eden/fs/model/Tree.h"
#include "eden/fuse/BufVec.h"

namespace facebook {
namespace eden {

class Blob;
class LocalStore;

class FileData {
 public:
  FileData(
      std::mutex& mutex,
      std::shared_ptr<LocalStore> store,
      const TreeEntry* entry);

  fusell::BufVec read(size_t size, off_t off);

 private:
  // Reference to the mutex in the associated inode.
  // It must be held by readers and writers before interpreting the filedata,
  // as any actor may cause materialization or truncation of the data.
  // Recommended practice in the implementation of methods on this class is to
  // hold a unique_lock as a guard for the duration of the method.
  std::mutex& mutex_;
  std::shared_ptr<LocalStore> store_;
  const TreeEntry* entry_;

  /// if backed by tree, the data from the tree, else nullptr.
  std::unique_ptr<Blob> blob_;
};
}
}
