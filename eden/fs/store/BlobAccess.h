/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include <folly/futures/Future.h>
#include <memory>
#include "eden/fs/store/BlobCache.h"

namespace facebook {
namespace eden {

class Blob;
class Hash;
class IObjectStore;

/**
 * File access in Eden is stateless - we do not receive notifications from the
 * kernel for open() and close(). However, it's inefficient to load blobs from
 * cache for every read() request that makes into the edenfs process. Thus,
 * centralize blob access through this interface.
 *
 * TODO: To support large files, they should be split into a series of blobs,
 * and those blobs should get their own blob IDs, which could then be used
 * in this API. Splitting large blobs into a series of smaller blobs has the
 * benefit of helping bound Eden's memory usage here.
 */
class BlobAccess {
 public:
  /**
   * Creates a new BlobAccess with specified cache parameters.
   *
   * - cacheSizeBytes attempts to restrict the number of blobs kept in memory to
   *   the specified number of bytes.
   * - recentBlobCacheLength specifies how many recently-accessed blobs should
   *   be kept around, even if they are larger than cacheSizeBytes. This is
   *   important to avoid reloading large files.
   */
  BlobAccess(
      std::shared_ptr<IObjectStore> objectStore,
      std::shared_ptr<BlobCache> blobCache);
  ~BlobAccess();

  /**
   * Loads and returns the entire blob's contents.
   *
   * If the accessPolicy is NotNeededAgain, the associated blob will not be
   * cached.
   *
   * Returns both the blob and an interest handle from the BlobCache that can
   * be dropped when the blob is no longer needed.
   */
  folly::Future<BlobCache::GetResult> getBlob(
      const Hash& hash,
      BlobCache::Interest interest = BlobCache::Interest::LikelyNeededAgain);

 private:
  BlobAccess(const BlobAccess&) = delete;
  BlobAccess& operator=(const BlobAccess&) = delete;

  const std::shared_ptr<IObjectStore> objectStore_;
  const std::shared_ptr<BlobCache> blobCache_;
};

} // namespace eden
} // namespace facebook
