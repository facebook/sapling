/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "ObjectStore.h"

#include <folly/Conv.h>
#include <folly/Format.h>
#include <folly/futures/Future.h>
#include <folly/io/IOBuf.h>
#include <folly/logging/xlog.h>
#include <stdexcept>

#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/BackingStore.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/tracing/EdenStats.h"

using folly::Future;
using folly::IOBuf;
using folly::makeFuture;
using std::shared_ptr;
using std::string;
using std::unique_ptr;

namespace facebook {
namespace eden {

std::shared_ptr<ObjectStore> ObjectStore::create(
    shared_ptr<LocalStore> localStore,
    shared_ptr<BackingStore> backingStore,
    shared_ptr<EdenStats> stats) {
  return std::shared_ptr<ObjectStore>{new ObjectStore{
      std::move(localStore), std::move(backingStore), std::move(stats)}};
}

ObjectStore::ObjectStore(
    shared_ptr<LocalStore> localStore,
    shared_ptr<BackingStore> backingStore,
    shared_ptr<EdenStats> stats)
    : metadataCache_{folly::in_place, kCacheSize},
      localStore_{std::move(localStore)},
      backingStore_{std::move(backingStore)},
      stats_{std::move(stats)} {}

ObjectStore::~ObjectStore() {}

Future<shared_ptr<const Tree>> ObjectStore::getTree(const Hash& id) const {
  // Check in the LocalStore first
  return localStore_->getTree(id).thenValue(
      [id, backingStore = backingStore_](shared_ptr<const Tree> tree) {
        if (tree) {
          XLOG(DBG4) << "tree " << id << " found in local store";
          return makeFuture(std::move(tree));
        }

        // Note: We don't currently have logic here to avoid duplicate work if
        // multiple callers request the same tree at once.  We could store a map
        // of pending lookups as (Hash --> std::list<Promise<unique_ptr<Tree>>),
        // and just add a new Promise to the list if this Hash already exists in
        // the pending list.
        //
        // However, de-duplication of object loads will already be done at the
        // Inode layer.  Therefore we currently don't bother de-duping loads at
        // this layer.

        // Load the tree from the BackingStore.
        return backingStore->getTree(id).thenValue(
            [id](unique_ptr<const Tree> loadedTree) {
              if (!loadedTree) {
                // TODO: Perhaps we should do some short-term negative caching?
                XLOG(DBG2) << "unable to find tree " << id;
                throw std::domain_error(
                    folly::to<string>("tree ", id.toString(), " not found"));
              }

              // TODO: For now, the BackingStore objects actually end up already
              // saving the Tree object in the LocalStore, so we don't do
              // anything here.
              //
              // localStore_->putTree(loadedTree.get());
              XLOG(DBG3) << "tree " << id << " retrieved from backing store";
              return shared_ptr<const Tree>(std::move(loadedTree));
            });
      });
}

Future<shared_ptr<const Tree>> ObjectStore::getTreeForCommit(
    const Hash& commitID) const {
  XLOG(DBG3) << "getTreeForCommit(" << commitID << ")";

  return backingStore_->getTreeForCommit(commitID).thenValue(
      [commitID](std::shared_ptr<const Tree> tree) {
        if (!tree) {
          throw std::domain_error(folly::to<string>(
              "unable to import commit ", commitID.toString()));
        }

        // For now we assume that the BackingStore will insert the Tree into the
        // LocalStore on its own, so we don't have to update the LocalStore
        // ourselves here.
        return tree;
      });
}

folly::Future<folly::Unit> ObjectStore::prefetchBlobs(
    const std::vector<Hash>& ids) const {
  // In theory we could/should ask the localStore_ to filter the list
  // of ids down to just the set that we need to load, but there is no
  // bulk key existence check in rocksdb, so we would need to cause it
  // to load all the blocks of those keys into memory.
  // So for the moment we are committing a layering violation in the
  // interest of making things faster in practice by just asking the
  // mercurial backing store to ensure that its local hgcache storage
  // has entries for all of the requested keys.
  if (ids.empty()) {
    return folly::unit;
  }
  return backingStore_->prefetchBlobs(ids);
}

Future<shared_ptr<const Blob>> ObjectStore::getBlob(const Hash& id) const {
  auto self = shared_from_this();

  return localStore_->getBlob(id).thenValue([id, self](
                                                shared_ptr<const Blob> blob) {
    if (blob) {
      // Not computing the BlobMetadata here because if the blob was found
      // in the local store, the LocalStore probably also has the metadata
      // already, and the caller may not even need the SHA-1 here. (If the
      // caller needed the SHA-1, they would have called getBlobMetadata
      // instead.)
      XLOG(DBG4) << "blob " << id << " found in local store";
      self->updateBlobStats(true, false);
      return makeFuture(shared_ptr<const Blob>(std::move(blob)));
    }

    // Look in the BackingStore
    return self->backingStore_->getBlob(id).thenValue(
        [self, id](unique_ptr<const Blob> loadedBlob) {
          if (loadedBlob) {
            XLOG(DBG3) << "blob " << id << "  retrieved from backing store";
            self->updateBlobStats(false, true);
            auto metadata = self->localStore_->putBlob(id, loadedBlob.get());
            self->metadataCache_.wlock()->set(id, metadata);
            return shared_ptr<const Blob>(std::move(loadedBlob));
          }

          XLOG(DBG2) << "unable to find blob " << id;
          self->updateBlobStats(false, false);
          // TODO: Perhaps we should do some short-term negative caching?
          throw std::domain_error(
              folly::to<string>("blob ", id.toString(), " not found"));
        });
  });
}

void ObjectStore::updateBlobStats(bool local, bool backing) const {
#if defined(EDEN_HAVE_STATS)
  ObjectStoreThreadStats& stats = stats_->getObjectStoreStatsForCurrentThread();
  stats.getBlobFromLocalStore.addValue(local);
  stats.getBlobFromBackingStore.addValue(backing);
#endif
}

Future<BlobMetadata> ObjectStore::getBlobMetadata(const Hash& id) const {
  // Check in-memory cache
  {
    auto metadataCache = metadataCache_.wlock();
    auto cacheIter = metadataCache->find(id);
    if (cacheIter != metadataCache->end()) {
      updateBlobMetadataStats(true, false, false);
      return cacheIter->second;
    }
  }

  auto self = shared_from_this();

  // Check local store
  return localStore_->getBlobMetadata(id).thenValue(
      [self, id](std::optional<BlobMetadata>&& metadata) {
        if (metadata) {
          self->updateBlobMetadataStats(false, true, false);
          self->metadataCache_.wlock()->set(id, *metadata);
          return makeFuture(*metadata);
        }

        // Check backing store
        //
        // TODO: It would be nice to add a smarter API to the BackingStore so
        // that we can query it just for the blob metadata if it supports
        // getting that without retrieving the full blob data.
        //
        // TODO: This should probably check the LocalStore for the blob first,
        // especially when we begin to expire entries in RocksDB.
        return self->backingStore_->getBlob(id).thenValue(
            [self, id](std::unique_ptr<Blob> blob) {
              if (blob) {
                self->updateBlobMetadataStats(false, false, true);
                auto metadata = self->localStore_->putBlob(id, blob.get());
                self->metadataCache_.wlock()->set(id, metadata);
                return makeFuture(metadata);
              }

              self->updateBlobMetadataStats(false, false, false);
              throw std::domain_error(
                  folly::to<string>("blob ", id.toString(), " not found"));
            });
      });
}

void ObjectStore::updateBlobMetadataStats(bool memory, bool local, bool backing)
    const {
#if defined(EDEN_HAVE_STATS)
  ObjectStoreThreadStats& stats = stats_->getObjectStoreStatsForCurrentThread();
  stats.getBlobMetadataFromMemory.addValue(memory);
  stats.getBlobMetadataFromLocalStore.addValue(local);
  stats.getBlobMetadataFromBackingStore.addValue(backing);
#endif
}

Future<Hash> ObjectStore::getBlobSha1(const Hash& id) const {
  return getBlobMetadata(id).thenValue(
      [](const BlobMetadata& metadata) { return metadata.sha1; });
}

Future<uint64_t> ObjectStore::getBlobSize(const Hash& id) const {
  auto self = shared_from_this();

  // Check local store for size
  return self->localStore_->getBlobSize(id).thenValue(
      [self, id](std::optional<uint64_t> size) {
        if (size) {
          self->updateBlobSizeStats(true, false);
          self->localStore_->putBlobSize(id, *size);
          return makeFuture(*size);
        }

        // Check backing store for blob
        return self->backingStore_->getBlob(id).thenValue(
            [self, id](std::unique_ptr<Blob> blob) {
              if (blob) {
                const uint64_t size = blob.get()->getSize();
                self->updateBlobSizeStats(false, true);
                self->localStore_->putBlobWithoutMetadata(id, blob.get());
                self->localStore_->putBlobSize(id, size);

                return makeFuture(size);
              }

              // Not found
              self->updateBlobSizeStats(false, false);
              throw std::domain_error(
                  folly::to<string>("blob ", id.toString(), " not found"));
            });
      });
}

void ObjectStore::updateBlobSizeStats(bool local, bool backing) const {
#if defined(EDEN_HAVE_STATS)
  ObjectStoreThreadStats& stats = stats_->getObjectStoreStatsForCurrentThread();
  stats.getBlobSizeFromLocalStore.addValue(local);
  stats.getBlobSizeFromBackingStore.addValue(backing);
#endif
}

} // namespace eden
} // namespace facebook
