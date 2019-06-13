/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "ObjectStore.h"

#include <folly/Conv.h>
#include <folly/futures/Future.h>
#include <folly/io/IOBuf.h>
#include <folly/logging/xlog.h>
#include <stdexcept>

#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/BackingStore.h"
#include "eden/fs/store/LocalStore.h"

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
    shared_ptr<BackingStore> backingStore) {
  return std::shared_ptr<ObjectStore>{
      new ObjectStore{std::move(localStore), std::move(backingStore)}};
}

ObjectStore::ObjectStore(
    shared_ptr<LocalStore> localStore,
    shared_ptr<BackingStore> backingStore)
    : metadataCache_{folly::in_place, kMetadataCacheSize},
      localStore_{std::move(localStore)},
      backingStore_{std::move(backingStore)} {}

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

Future<shared_ptr<const Blob>> ObjectStore::getBlob(const Hash& id) const {
  return localStore_->getBlob(id).thenValue(
      [id, self = shared_from_this()](shared_ptr<const Blob> blob) {
        if (blob) {
          // Not computing the BlobMetadata here because if the blob was found
          // in the local store, the LocalStore probably also has the metadata
          // already, and the caller may not even need the SHA-1 here. (If the
          // caller needed the SHA-1, they would have called getBlobMetadata
          // instead.)
          XLOG(DBG4) << "blob " << id << "  found in local store";
          return makeFuture(shared_ptr<const Blob>(std::move(blob)));
        }

        // Look in the BackingStore
        return self->backingStore_->getBlob(id).thenValue(
            [self, id](unique_ptr<const Blob> loadedBlob) {
              if (!loadedBlob) {
                XLOG(DBG2) << "unable to find blob " << id;
                // TODO: Perhaps we should do some short-term negative caching?
                throw std::domain_error(
                    folly::to<string>("blob ", id.toString(), " not found"));
              }

              XLOG(DBG3) << "blob " << id << "  retrieved from backing store";
              auto metadata = self->localStore_->putBlob(id, loadedBlob.get());
              self->metadataCache_.wlock()->set(id, metadata);
              return shared_ptr<const Blob>(std::move(loadedBlob));
            });
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

Future<BlobMetadata> ObjectStore::getBlobMetadata(const Hash& id) const {
  // First, check the in-memory cache.
  {
    auto metadataCache = metadataCache_.wlock();
    auto cacheIter = metadataCache->find(id);
    if (cacheIter != metadataCache->end()) {
      return cacheIter->second;
    }
  }

  return localStore_->getBlobMetadata(id).thenValue(
      [id, self = shared_from_this()](std::optional<BlobMetadata>&& localData) {
        if (localData.has_value()) {
          self->metadataCache_.wlock()->set(id, localData.value());
          return makeFuture(localData.value());
        }

        // Load the blob from the BackingStore.
        //
        // TODO: It would be nice to add a smarter API to the BackingStore so
        // that we can query it just for the blob metadata if it supports
        // getting that without retrieving the full blob data.
        //
        // TODO: This should probably check the LocalStore for the blob first,
        // especially when we begin to expire entries in RocksDB.
        return self->backingStore_->getBlob(id).thenValue(
            [self, id](std::unique_ptr<Blob> blob) {
              if (!blob) {
                // TODO: Perhaps we should do some short-term negative caching?
                throw std::domain_error(
                    folly::to<string>("blob ", id.toString(), " not found"));
              }

              auto metadata = self->localStore_->putBlob(id, blob.get());
              self->metadataCache_.wlock()->set(id, metadata);
              return metadata;
            });
      });
}

Future<size_t> ObjectStore::getSize(const Hash& id) const {
  return getBlobMetadata(id).thenValue(
      [](const BlobMetadata& metadata) { return metadata.size; });
}

Future<Hash> ObjectStore::getSha1(const Hash& id) const {
  return getBlobMetadata(id).thenValue(
      [](const BlobMetadata& metadata) { return metadata.sha1; });
}

} // namespace eden
} // namespace facebook
