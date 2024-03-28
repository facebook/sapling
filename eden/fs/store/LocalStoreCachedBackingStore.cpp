/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/LocalStoreCachedBackingStore.h"
#include "eden/common/utils/ImmediateFuture.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/BlobMetadata.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/telemetry/EdenStats.h"

namespace facebook::eden {

LocalStoreCachedBackingStore::LocalStoreCachedBackingStore(
    std::shared_ptr<BackingStore> backingStore,
    std::shared_ptr<LocalStore> localStore,
    EdenStatsPtr stats,
    LocalStoreCachedBackingStore::CachingPolicy cachingPolicy)
    : backingStore_{std::move(backingStore)},
      localStore_{std::move(localStore)},
      stats_{std::move(stats)},
      cachingPolicy_{cachingPolicy} {
  XCHECK_NE(
      cachingPolicy_, LocalStoreCachedBackingStore::CachingPolicy::NoCaching);
}

LocalStoreCachedBackingStore::~LocalStoreCachedBackingStore() = default;

ObjectComparison LocalStoreCachedBackingStore::compareObjectsById(
    const ObjectId& one,
    const ObjectId& two) {
  return backingStore_->compareObjectsById(one, two);
}

ImmediateFuture<BackingStore::GetRootTreeResult>
LocalStoreCachedBackingStore::getRootTree(
    const RootId& rootId,
    const ObjectFetchContextPtr& context) {
  return backingStore_->getRootTree(rootId, context)
      .thenValue([localStore = localStore_](GetRootTreeResult result) {
        // TODO: perhaps this callback should use toUnsafeFuture() to ensure the
        // tree is cached whether or not the caller consumes the future.
        localStore->putTree(*result.tree);
        return result;
      });
}

ImmediateFuture<std::shared_ptr<TreeEntry>>
LocalStoreCachedBackingStore::getTreeEntryForObjectId(
    const ObjectId& objectId,
    TreeEntryType treeEntryType,
    const ObjectFetchContextPtr& context) {
  return backingStore_->getTreeEntryForObjectId(
      objectId, treeEntryType, context);
}

folly::SemiFuture<BackingStore::GetTreeResult>
LocalStoreCachedBackingStore::getTree(
    const ObjectId& id,
    const ObjectFetchContextPtr& context) {
  auto localStoreGetTree = ImmediateFuture<TreePtr>{std::in_place, nullptr};
  if (shouldCache(LocalStoreCachedBackingStore::CachingPolicy::Trees)) {
    localStoreGetTree = localStore_->getTree(id);
  }

  return std::move(localStoreGetTree)
      .thenValue(
          [self = shared_from_this(), id = id, context = context.copy()](
              TreePtr tree) mutable
          -> ImmediateFuture<BackingStore::GetTreeResult> {
            if (tree) {
              self->stats_->increment(&ObjectStoreStats::getTreeFromLocalStore);
              return GetTreeResult{
                  std::move(tree), ObjectFetchContext::FromDiskCache};
            }

            return ImmediateFuture{self->backingStore_->getTree(id, context)}
                // TODO: This is a good use for toUnsafeFuture to ensure the
                // tree is cached even if the resulting future is never
                // consumed.
                .thenValue([self](GetTreeResult result) {
                  if (result.tree) {
                    auto batch = self->localStore_->beginWrite();
                    if (self->shouldCache(LocalStoreCachedBackingStore::
                                              CachingPolicy::Trees)) {
                      batch->putTree(*result.tree);
                    }

                    if (self->shouldCache(LocalStoreCachedBackingStore::
                                              CachingPolicy::BlobMetadata)) {
                      // Let's cache all the entries in the LocalStore.
                      for (const auto& [name, treeEntry] : *result.tree) {
                        const auto& size = treeEntry.getSize();
                        const auto& sha1 = treeEntry.getContentSha1();
                        const auto& blake3 = treeEntry.getContentBlake3();
                        if (treeEntry.getType() ==
                                TreeEntryType::REGULAR_FILE &&
                            size && sha1) {
                          batch->putBlobMetadata(
                              treeEntry.getHash(),
                              BlobMetadata{*sha1, blake3, *size});
                        }
                      }
                    }
                    batch->flush();
                    self->stats_->increment(
                        &ObjectStoreStats::getTreeFromBackingStore);
                  }

                  return result;
                });
          })
      .semi();
}

folly::SemiFuture<BackingStore::GetBlobMetaResult>
LocalStoreCachedBackingStore::getBlobMetadata(
    const ObjectId& id,
    const ObjectFetchContextPtr& context) {
  auto localStoreGetBlobMetadata =
      ImmediateFuture<BlobMetadataPtr>{std::in_place, nullptr};
  if (shouldCache(LocalStoreCachedBackingStore::CachingPolicy::BlobMetadata)) {
    localStoreGetBlobMetadata = localStore_->getBlobMetadata(id);
  }
  return std::move(localStoreGetBlobMetadata)
      .thenValue(
          [self = shared_from_this(), id = id, context = context.copy()](
              BlobMetadataPtr metadata) mutable
          -> ImmediateFuture<BackingStore::GetBlobMetaResult> {
            if (metadata) {
              self->stats_->increment(
                  &ObjectStoreStats::getBlobMetadataFromLocalStore);
              return GetBlobMetaResult{
                  std::move(metadata), ObjectFetchContext::FromDiskCache};
            }

            return ImmediateFuture{
                self->backingStore_->getBlobMetadata(id, context)}
                .thenValue(
                    [self, id, context = context.copy()](
                        GetBlobMetaResult result)
                        -> ImmediateFuture<GetBlobMetaResult> {
                      if (result.blobMeta &&
                          result.blobMeta->sha1 !=
                              kZeroHash) { // from eden/fs/model/Hash.cpp
                        if (result.origin ==
                            ObjectFetchContext::Origin::FromDiskCache) {
                          self->stats_->increment(
                              &ObjectStoreStats::
                                  getLocalBlobMetadataFromBackingStore);
                        } else {
                          self->stats_->increment(
                              &ObjectStoreStats::
                                  getBlobMetadataFromBackingStore);
                        }

                        return result;
                      }

                      return ImmediateFuture{self->getBlob(id, context)}
                          .thenValue([self,
                                      backingStoreResult = std::move(result)](
                                         GetBlobResult result) {
                            if (result.blob) {
                              self->stats_->increment(
                                  &ObjectStoreStats::getBlobMetadataFromBlob);

                              std::optional<Hash32> blake3;
                              if (backingStoreResult.blobMeta &&
                                  backingStoreResult.blobMeta->blake3
                                      .has_value()) {
                                blake3 =
                                    backingStoreResult.blobMeta->blake3.value();
                              }

                              return GetBlobMetaResult{
                                  std::make_shared<BlobMetadata>(
                                      Hash20::sha1(result.blob->getContents()),
                                      std::move(blake3),
                                      result.blob->getSize()),
                                  result.origin};
                            }

                            return GetBlobMetaResult{
                                nullptr,
                                ObjectFetchContext::Origin::NotFetched};
                          });
                    })
                .thenValue([self, id](GetBlobMetaResult result) {
                  if (result.blobMeta &&
                      self->shouldCache(LocalStoreCachedBackingStore::
                                            CachingPolicy::BlobMetadata)) {
                    self->localStore_->putBlobMetadata(id, *result.blobMeta);
                  }
                  return result;
                });
          })
      .semi();
}

folly::SemiFuture<BackingStore::GetBlobResult>
LocalStoreCachedBackingStore::getBlob(
    const ObjectId& id,
    const ObjectFetchContextPtr& context) {
  auto localStoreGetBlob = ImmediateFuture<BlobPtr>{std::in_place, nullptr};
  if (shouldCache(LocalStoreCachedBackingStore::CachingPolicy::Blobs)) {
    localStoreGetBlob = localStore_->getBlob(id);
  }
  return std::move(localStoreGetBlob)
      .thenValue(
          [self = shared_from_this(), id = id, context = context.copy()](
              BlobPtr blob) mutable
          -> ImmediateFuture<BackingStore::GetBlobResult> {
            if (blob) {
              self->stats_->increment(&ObjectStoreStats::getBlobFromLocalStore);
              return GetBlobResult{
                  std::move(blob), ObjectFetchContext::FromDiskCache};
            }

            return ImmediateFuture{self->backingStore_->getBlob(id, context)}
                // TODO: This is a good use for toUnsafeFuture to ensure the
                // tree is cached even if the resulting future is never
                // consumed.
                .thenValue([self, id](GetBlobResult result) {
                  if (result.blob) {
                    if (self->shouldCache(LocalStoreCachedBackingStore::
                                              CachingPolicy::Blobs)) {
                      self->localStore_->putBlob(id, result.blob.get());
                    }
                    self->stats_->increment(
                        &ObjectStoreStats::getBlobFromBackingStore);
                  }
                  return result;
                });
          })
      .semi();
}

folly::SemiFuture<folly::Unit> LocalStoreCachedBackingStore::prefetchBlobs(
    ObjectIdRange ids,
    const ObjectFetchContextPtr& context) {
  return backingStore_->prefetchBlobs(ids, context);
}

void LocalStoreCachedBackingStore::periodicManagementTask() {
  backingStore_->periodicManagementTask();
}

void LocalStoreCachedBackingStore::startRecordingFetch() {
  backingStore_->startRecordingFetch();
}

std::unordered_set<std::string>
LocalStoreCachedBackingStore::stopRecordingFetch() {
  return backingStore_->stopRecordingFetch();
}

ImmediateFuture<folly::Unit>
LocalStoreCachedBackingStore::importManifestForRoot(
    const RootId& rootId,
    const Hash20& manifest,
    const ObjectFetchContextPtr& context) {
  return backingStore_->importManifestForRoot(rootId, manifest, context);
}

RootId LocalStoreCachedBackingStore::parseRootId(folly::StringPiece rootId) {
  return backingStore_->parseRootId(rootId);
}

std::string LocalStoreCachedBackingStore::renderRootId(const RootId& rootId) {
  return backingStore_->renderRootId(rootId);
}

ObjectId LocalStoreCachedBackingStore::parseObjectId(
    folly::StringPiece objectId) {
  return backingStore_->parseObjectId(objectId);
}

std::string LocalStoreCachedBackingStore::renderObjectId(
    const ObjectId& objectId) {
  return backingStore_->renderObjectId(objectId);
}

std::optional<folly::StringPiece> LocalStoreCachedBackingStore::getRepoName() {
  return backingStore_->getRepoName();
}

bool LocalStoreCachedBackingStore::shouldCache(CachingPolicy object) const {
  auto underlyingObject = folly::to_underlying(object);
  return (folly::to_underlying(cachingPolicy_) & underlyingObject) ==
      underlyingObject;
}

} // namespace facebook::eden
