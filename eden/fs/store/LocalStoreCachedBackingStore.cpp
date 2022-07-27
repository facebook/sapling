/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/LocalStoreCachedBackingStore.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/utils/ImmediateFuture.h"

namespace facebook::eden {

LocalStoreCachedBackingStore::LocalStoreCachedBackingStore(
    std::shared_ptr<BackingStore> backingStore,
    std::shared_ptr<LocalStore> localStore,
    std::shared_ptr<EdenStats> stats)
    : backingStore_{std::move(backingStore)},
      localStore_{std::move(localStore)},
      stats_{std::move(stats)} {}

LocalStoreCachedBackingStore::~LocalStoreCachedBackingStore() {}

ObjectComparison LocalStoreCachedBackingStore::compareObjectsById(
    const ObjectId& one,
    const ObjectId& two) {
  return backingStore_->compareObjectsById(one, two);
}

folly::SemiFuture<std::unique_ptr<Tree>>
LocalStoreCachedBackingStore::getRootTree(
    const RootId& rootId,
    ObjectFetchContext& context) {
  return backingStore_->getRootTree(rootId, context)
      .deferValue([localStore = localStore_](std::unique_ptr<Tree> tree) {
        if (tree) {
          localStore->putTree(*tree);
        }
        return tree;
      });
}

folly::SemiFuture<std::unique_ptr<TreeEntry>>
LocalStoreCachedBackingStore::getTreeEntryForRootId(
    const RootId& rootId,
    TreeEntryType treeEntryType,
    ObjectFetchContext& context) {
  return backingStore_->getTreeEntryForRootId(rootId, treeEntryType, context);
}

folly::SemiFuture<BackingStore::GetTreeRes>
LocalStoreCachedBackingStore::getTree(
    const ObjectId& id,
    ObjectFetchContext& context) {
  return localStore_->getTree(id)
      .thenValue(
          [id = id,
           &context,
           localStore = localStore_,
           backingStore = backingStore_](std::unique_ptr<Tree> tree) mutable {
            if (tree) {
              return folly::makeSemiFuture(BackingStore::GetTreeRes{
                  std::move(tree), ObjectFetchContext::FromDiskCache});
            }

            return backingStore->getTree(id, context)
                .deferValue([localStore = std::move(localStore)](
                                BackingStore::GetTreeRes result) {
                  if (result.tree) {
                    localStore->putTree(*result.tree);
                  }

                  return result;
                });
          })
      .semi();
}

std::unique_ptr<BlobMetadata>
LocalStoreCachedBackingStore::getLocalBlobMetadata(
    const ObjectId& id,
    ObjectFetchContext& context) {
  return backingStore_->getLocalBlobMetadata(id, context);
}

folly::SemiFuture<BackingStore::GetBlobRes>
LocalStoreCachedBackingStore::getBlob(
    const ObjectId& id,
    ObjectFetchContext& context) {
  return localStore_->getBlob(id)
      .thenValue([id = id,
                  &context,
                  localStore = localStore_,
                  backingStore = backingStore_,
                  stats = stats_](std::unique_ptr<Blob> blob) mutable {
        if (blob) {
          stats->getObjectStoreStatsForCurrentThread()
              .getBlobFromLocalStore.addValue(1);
          return folly::makeSemiFuture(BackingStore::GetBlobRes{
              std::move(blob), ObjectFetchContext::FromDiskCache});
        }

        return backingStore->getBlob(id, context)
            .deferValue([localStore = std::move(localStore),
                         stats = std::move(stats),
                         id](BackingStore::GetBlobRes result) {
              if (result.blob) {
                localStore->putBlob(id, result.blob.get());
                stats->getObjectStoreStatsForCurrentThread()
                    .getBlobFromBackingStore.addValue(1);
              }
              return result;
            });
      })
      .semi();
}

folly::SemiFuture<folly::Unit> LocalStoreCachedBackingStore::prefetchBlobs(
    ObjectIdRange ids,
    ObjectFetchContext& context) {
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

folly::SemiFuture<folly::Unit>
LocalStoreCachedBackingStore::importManifestForRoot(
    const RootId& rootId,
    const Hash20& manifest) {
  return backingStore_->importManifestForRoot(rootId, manifest);
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

} // namespace facebook::eden
