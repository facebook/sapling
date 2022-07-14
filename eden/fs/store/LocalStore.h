/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>
#include <atomic>
#include <memory>
#include <optional>
#include "eden/fs/model/BlobMetadata.h"
#include "eden/fs/model/ObjectId.h"
#include "eden/fs/store/KeySpace.h"
#include "eden/fs/utils/ImmediateFuture.h"
#include "eden/fs/utils/PathFuncs.h"

namespace folly {
template <typename T>
class Future;
} // namespace folly

namespace facebook::eden {

class Blob;
class EdenConfig;
class StoreResult;
class Tree;
class TreeMetadata;

/*
 * LocalStore stores objects (trees and blobs) locally on disk.
 *
 * This is a content-addressed store, so objects can be only retrieved using
 * their hash.
 *
 * The LocalStore was originally only a cache.  The intent was that If an
 * object is not found in the LocalStore then it will need to be retrieved
 * from the BackingStore.  The introduction of HgProxyHashFamily renders this
 * comment a little inaccurate because we don't have a way to produce the
 * required data if the proxy hash data has been removed.  We expect things
 * to revert back to a more pure cache as we evolve our interfaces with
 * Mercurial and Mononoke.
 *
 * LocalStore is thread-safe, and can be used from multiple threads without
 * requiring the caller to perform locking around accesses to the LocalStore.
 */
class LocalStore : public std::enable_shared_from_this<LocalStore> {
 public:
  LocalStore() = default;
  virtual ~LocalStore() = default;

  /**
   * Close the underlying store.
   */
  virtual void close() = 0;

  /**
   * Iterate through every KeySpace, clearing the ones that are deprecated.
   */
  void clearDeprecatedKeySpaces();

  /**
   * Iterate through every KeySpace, clearing the ones that are safe to forget
   * and compacting all of them.
   */
  void clearCachesAndCompactAll();

  /**
   * Delete every object from the store that cannot be repopulated from the
   * backing store. Notably, this does not include proxy hashes.
   */
  void clearCaches();

  /**
   * Compacts storage for all key spaces.
   */
  void compactStorage();

  /**
   * Clears all entries from the given KeySpace.
   */
  virtual void clearKeySpace(KeySpace keySpace) = 0;

  /**
   * Ask the storage engine to compact the KeySpace.
   */
  virtual void compactKeySpace(KeySpace keySpace) = 0;

  /**
   * Get arbitrary unserialized data from the store.
   *
   * StoreResult::isValid() will be true if the key was found, and false
   * if the key was not present.
   *
   * May throw exceptions on error.
   */
  virtual StoreResult get(KeySpace keySpace, folly::ByteRange key) const = 0;
  StoreResult get(KeySpace keySpace, const ObjectId& id) const;

  FOLLY_NODISCARD virtual ImmediateFuture<StoreResult> getImmediateFuture(
      KeySpace keySpace,
      const ObjectId& id) const;

  FOLLY_NODISCARD virtual folly::Future<std::vector<StoreResult>> getBatch(
      KeySpace keySpace,
      const std::vector<folly::ByteRange>& keys) const;

  /**
   * Get a Tree from the store.
   *
   * Returns nullptr if this key is not present in the store.
   * May throw exceptions on error (e.g., if this ID refers to a non-tree
   * object).
   */
  ImmediateFuture<std::unique_ptr<Tree>> getTree(const ObjectId& id) const;

  /**
   * Get a Blob from the store.
   *
   * Blob objects store file data.
   *
   * Returns nullptr if this key is not present in the store.
   * May throw exceptions on error (e.g., if this ID refers to a non-blob
   * object).
   */
  ImmediateFuture<std::unique_ptr<Blob>> getBlob(const ObjectId& id) const;

  /**
   * Get the size of a blob and the SHA-1 hash of its contents.
   *
   * Returns std::nullopt if this key is not present in the store, or throws an
   * exception on error.
   */
  ImmediateFuture<std::optional<BlobMetadata>> getBlobMetadata(
      const ObjectId& id) const;

  /**
   * Test whether the key is stored.
   */
  virtual bool hasKey(KeySpace keySpace, folly::ByteRange key) const = 0;
  bool hasKey(KeySpace keySpace, const ObjectId& id) const;

  /**
   * Store a Tree into the TreeFamily KeySpace.
   */
  void putTree(const Tree& tree);

  /**
   * Store a Blob.
   */
  void putBlob(const ObjectId& id, const Blob* blob);

  /**
   * Store a blob metadata.
   */
  BlobMetadata putBlobMetadata(const ObjectId& id, const Blob* blob);

  /**
   * Put arbitrary data in the store.
   */
  virtual void
  put(KeySpace keySpace, folly::ByteRange key, folly::ByteRange value) = 0;
  void put(KeySpace keySpace, const ObjectId& id, folly::ByteRange value);

  /*
   * WriteBatch is a helper class for facilitating a bulk store operation.
   *
   * The purpose of this class is to let multiple callers manage independent
   * write batches and flush them to the backing storage when its deemed
   * appropriate.
   *
   * WriteBatch is not safe to mutate from multiple threads concurrently.
   *
   * Typical usage:
   * auto writer = localStore->beginWrite();
   * writer->put(KeySpace::Meta, Key, Value);
   * writer->put(KeySpace::Blob, Key, BlobValue);
   * writer->flush();
   */
  class WriteBatch {
   public:
    /**
     * Store a Tree into the TreeFamily KeySpace.
     */
    void putTree(const Tree& tree);

    /**
     * Store a Blob.
     */
    void putBlob(const ObjectId& id, const Blob* blob);

    /**
     * Put arbitrary data in the store.
     */
    virtual void
    put(KeySpace keySpace, folly::ByteRange key, folly::ByteRange value) = 0;
    void put(KeySpace keySpace, const ObjectId& id, folly::ByteRange value);

    /**
     * Put arbitrary data in the store where the value is split across
     * a set of sliced data.
     */
    virtual void put(
        KeySpace keySpace,
        folly::ByteRange key,
        std::vector<folly::ByteRange> valueSlices) = 0;

    /**
     * Flush any pending data to the store.
     */
    virtual void flush() = 0;

    // Forbidden copy construction/assignment; allow only moves
    WriteBatch(const WriteBatch&) = delete;
    WriteBatch(WriteBatch&&) = default;
    WriteBatch& operator=(const WriteBatch&) = delete;
    WriteBatch& operator=(WriteBatch&&) = default;
    virtual ~WriteBatch();
    WriteBatch() = default;

   private:
    friend class LocalStore;
  };

  /**
   * Construct a LocalStoreBatchWrite object with write batch of size bufSize.
   * If bufSize is non-zero the batch will automatically flush each time
   * the accumulated data exceeds bufSize.  Otherwise no implifict flushing
   * will occur.
   * Either way, the caller will typically want to finish up by calling
   * writeBatch->flush() to complete the batch as there is no implicit flush on
   * destruction either.
   */
  virtual std::unique_ptr<WriteBatch> beginWrite(size_t bufSize = 0) = 0;

  virtual void periodicManagementTask(const EdenConfig& config);

  /*
   * We keep this field to avoid making `LocalStore` holding a reference to
   * `EdenConfig`, which will require us to change all the subclasses. We update
   * this flag through `periodicManagementTask` function. The implication is
   * that the configuration may need up to 1 minute to propagate (or whatever
   * the configured local store management interval is).
   */
  std::atomic<bool> enableBlobCaching = true;

 private:
  /**
   * Compute the serialized version of the tree in a (not coalesced) IOBuf.
   * This does not modify the contents of the store; it is the method
   * used by the putTree method to compute the data that it stores.
   * This is useful when computing the overall set of data during a
   * two phase import.
   */
  static folly::IOBuf serializeTree(const Tree& tree);
};

} // namespace facebook::eden
