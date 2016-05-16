/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "FileData.h"

#include <folly/io/Cursor.h>
#include "eden/fs/store/LocalStore.h"

namespace facebook {
namespace eden {

FileData::FileData(
    std::mutex& mutex,
    std::shared_ptr<LocalStore> store,
    const TreeEntry* entry)
    : mutex_(mutex), store_(store), entry_(entry) {
  if (entry_) {
    blob_ = store_->getBlob(entry_->getHash());
  }
}

fusell::BufVec FileData::read(size_t size, off_t off) {
  std::unique_lock<std::mutex> lock(mutex_);

  auto buf = blob_->getContents();
  folly::io::Cursor cursor(&buf);

  if (!cursor.canAdvance(off)) {
    // Seek beyond EOF.  Return an empty result.
    return fusell::BufVec(folly::IOBuf::wrapBuffer("", 0));
  }

  cursor.skip(off);

  std::unique_ptr<folly::IOBuf> result;
  cursor.cloneAtMost(result, size);
  return fusell::BufVec(std::move(result));
}
}
}
