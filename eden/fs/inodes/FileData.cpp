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

#include <folly/Exception.h>
#include <folly/FileUtil.h>
#include <folly/io/Cursor.h>
#include "eden/fs/overlay/Overlay.h"
#include "eden/fs/store/LocalStore.h"

namespace facebook {
namespace eden {

FileData::FileData(
    std::mutex& mutex,
    std::shared_ptr<LocalStore> store,
    std::shared_ptr<Overlay> overlay,
    const TreeEntry* entry)
    : mutex_(mutex), store_(store), overlay_(overlay), entry_(entry) {}

fusell::BufVec FileData::read(size_t size, off_t off) {
  std::unique_lock<std::mutex> lock(mutex_);

  // Temporary, pending the changes in a following diff
  CHECK(blob_);

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

void FileData::materialize(int open_flags, RelativePathPiece path) {
  std::unique_lock<std::mutex> lock(mutex_);

  // If we have a tree entry, assume that we will need the blob data...
  bool need_blob = entry_ != nullptr;
  // ... and that we don't need an overlay file handle.
  bool need_file = false;

  if ((open_flags & O_TRUNC) != 0) {
    // Truncation is a write operation, so we will need an overlay file.
    need_file = true;
    // No need to materialize the blob from the store if we're just
    // going to truncate it anyway.
    need_blob = false;
  }
  if ((open_flags & (O_RDWR | O_WRONLY)) != 0) {
    // Write operations require an overlay file.
    need_file = true;
  }

  if (need_blob && overlay_->isWhiteout(path)) {
    // Data was deleted, no need to go to the store to satisfy it.
    need_blob = false;
  }

  // If we have a pre-existing overlay file, we do not need to go to
  // the store at all.
  if (!file_) {
    try {
      // Test whether an overlay file exists by trying to open it.
      file_ = overlay_->openFile(path, O_RDWR, 0600);
      // since we have a pre-existing overlay file, we don't need the blob.
      need_blob = false;
    } catch (const std::system_error& err) {
      if (err.code().value() != ENOENT) {
        throw;
      }
      // Else: doesn't exist in the overlay right now
    }
  }

  if (need_blob && !blob_) {
    // Load the blob data.
    blob_ = store_->getBlob(entry_->getHash());
  }

  if (need_file && !file_) {
    if (!entry_ && (open_flags & O_CREAT) == 0) {
      // If we get here, we do not have any usable backing from the store
      // and do not have a pre-existing overlay file.
      // The current file open request isn't asking us to create a file,
      // therefore we should not create one as we are about to do below.
      // I don't know if the kernel is smart enough to detect and prevent
      // this at a higher level or not, but it feels safer to be sure here.
      folly::throwSystemErrorExplicit(ENOENT);
    }

    // We need an overlay file and don't yet have one.
    // We always create our internal file handle read/write regardless of
    // the mode that the client is requesting.
    auto file = overlay_->openFile(path, O_RDWR | O_CREAT, 0600);

    // We typically need to populate our newly opened file with the data
    // from the overlay.  The O_TRUNC check above may have set need_blob
    // to false, so we'll end up just creating an empty file and skipping
    // the blob copy.
    if (need_blob) {
      auto buf = blob_->getContents();
      auto iov = buf.getIov();
      auto wrote = folly::writevNoInt(file.fd(), iov.data(), iov.size());
      auto err = errno;
      if (wrote != buf.computeChainDataLength()) {
        folly::throwSystemErrorExplicit(
            wrote == -1 ? err : EIO, "failed to materialize ", path);
      }
    }

    // transfer ownership of the fd to us.  Do this after dealing with any
    // errors during materialization so that our internal state is easier
    // to reason about.
    file_ = std::move(file);
  } else if (file_ && (open_flags & O_TRUNC) != 0) {
    // truncating a file that we already have open
    folly::checkUnixError(ftruncate(file_.fd(), 0));
  }
}
}
}
