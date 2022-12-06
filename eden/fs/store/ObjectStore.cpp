/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "ObjectStore.h"

#include <folly/Conv.h>
#include <folly/futures/Future.h>
#include <folly/io/IOBuf.h>

#include <stdexcept>

#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/BackingStore.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/utils/ImmediateFuture.h"
#include "eden/fs/utils/Throw.h"

using folly::Future;
using folly::makeFuture;
using std::shared_ptr;
using std::string;
using std::unique_ptr;

namespace facebook::eden {

namespace {
constexpr uint64_t kImportPriorityDeprioritizeAmount = 1;
}

std::shared_ptr<ObjectStore> ObjectStore::create(
    shared_ptr<LocalStore> localStore,
    shared_ptr<BackingStore> backingStore,
    shared_ptr<TreeCache> treeCache,
    shared_ptr<EdenStats> stats,
    std::shared_ptr<ProcessNameCache> processNameCache,
    std::shared_ptr<StructuredLogger> structuredLogger,
    std::shared_ptr<const EdenConfig> edenConfig,
    CaseSensitivity caseSensitive) {
  return std::shared_ptr<ObjectStore>{new ObjectStore{
      std::move(localStore),
      std::move(backingStore),
      std::move(treeCache),
      std::move(stats),
      processNameCache,
      structuredLogger,
      edenConfig,
      caseSensitive}};
}

ObjectStore::ObjectStore(
    shared_ptr<LocalStore> localStore,
    shared_ptr<BackingStore> backingStore,
    shared_ptr<TreeCache> treeCache,
    shared_ptr<EdenStats> stats,
    std::shared_ptr<ProcessNameCache> processNameCache,
    std::shared_ptr<StructuredLogger> structuredLogger,
    std::shared_ptr<const EdenConfig> edenConfig,
    CaseSensitivity caseSensitive)
    : metadataCache_{folly::in_place, kCacheSize},
      treeCache_{std::move(treeCache)},
      localStore_{std::move(localStore)},
      backingStore_{std::move(backingStore)},
      stats_{std::move(stats)},
      pidFetchCounts_{std::make_unique<PidFetchCounts>()},
      processNameCache_(processNameCache),
      structuredLogger_(structuredLogger),
      edenConfig_(edenConfig),
      caseSensitive_{caseSensitive} {
  XCHECK(localStore_);
  XCHECK(backingStore_);
  XCHECK(stats_);
}

ObjectStore::~ObjectStore() {}

void ObjectStore::updateProcessFetch(
    const ObjectFetchContext& fetchContext) const {
  if (auto pid = fetchContext.getClientPid()) {
    auto fetch_count = pidFetchCounts_->recordProcessFetch(pid.value());
    auto threshold = edenConfig_->fetchHeavyThreshold.getValue();
    // indicate heavy event when fetch_count reaches multiple of threshold
    if (fetch_count && threshold && (fetch_count % threshold) == 0) {
      sendFetchHeavyEvent(pid.value(), fetch_count);
    }
  }
}

void ObjectStore::sendFetchHeavyEvent(pid_t pid, uint64_t fetch_count) const {
  auto processName = processNameCache_->getProcessName(pid);
  if (processName) {
    std::replace(processName->begin(), processName->end(), '\0', ' ');
    XLOG(WARN) << "Heavy fetches (" << fetch_count << ") from process "
               << *processName << "(pid=" << pid << ")";
    structuredLogger_->logEvent(
        FetchHeavy{processName.value(), pid, fetch_count});
  } else {
    XLOG(WARN) << "Heavy fetches (" << fetch_count << ") from pid " << pid
               << ")";
  }
}

void ObjectStore::deprioritizeWhenFetchHeavy(
    ObjectFetchContext& context) const {
  auto pid = context.getClientPid();
  if (pid.has_value()) {
    auto fetch_count = pidFetchCounts_->getCountByPid(pid.value());
    auto threshold = edenConfig_->fetchHeavyThreshold.getValue();
    if (threshold && fetch_count >= threshold) {
      context.deprioritize(kImportPriorityDeprioritizeAmount);
    }
  }
}

RootId ObjectStore::parseRootId(folly::StringPiece rootId) {
  return backingStore_->parseRootId(rootId);
}

std::string ObjectStore::renderRootId(const RootId& rootId) {
  return backingStore_->renderRootId(rootId);
}

ObjectId ObjectStore::parseObjectId(folly::StringPiece objectId) {
  return backingStore_->parseObjectId(objectId);
}

std::string ObjectStore::renderObjectId(const ObjectId& objectId) {
  return backingStore_->renderObjectId(objectId);
}

namespace {
/**
 * The passed in Tree may differ in case sensitivity from the ObjectStore's
 * case sensitivity. In that case, the Tree is copied and its case sensitivity
 * is switched.
 *
 * In practice, this conversion is extremely rare due to most mounts being
 * created with the default case sensitivity.
 *
 * TODO(xavierd): Is this ugly? Yes, but this will allow incrementally
 * converting the BackingStore+LocalStore+TreeCache to care about case
 * sensitivity separately.
 */
std::shared_ptr<const Tree> changeCaseSensitivity(
    std::shared_ptr<const Tree> tree,
    CaseSensitivity caseSensitive) {
  if (tree->getCaseSensitivity() == caseSensitive) {
    return tree;
  } else {
    auto treeEntries = Tree::container{
        tree->cbegin(), tree->cend(), caseSensitive}; // Explicit copy.
    return std::make_shared<const Tree>(
        std::move(treeEntries), tree->getHash());
  }
}

BlobMetadata computeBlobMetadata(const Blob& blob) {
  return BlobMetadata{Hash20::sha1(blob.getContents()), blob.getSize()};
}

} // namespace

ImmediateFuture<shared_ptr<const Tree>> ObjectStore::getRootTree(
    const RootId& rootId,
    const ObjectFetchContextPtr& context) const {
  XLOG(DBG3) << "getRootTree(" << rootId << ")";
  return ImmediateFuture{backingStore_->getRootTree(rootId, context)}.thenValue(
      [treeCache = treeCache_, rootId, caseSensitive = caseSensitive_](
          std::shared_ptr<const Tree> tree) {
        if (!tree) {
          throw_<std::domain_error>("unable to import root ", rootId);
        }

        treeCache->insert(tree);

        return changeCaseSensitivity(std::move(tree), caseSensitive);
      });
}

ImmediateFuture<std::shared_ptr<TreeEntry>>
ObjectStore::getTreeEntryForObjectId(
    const ObjectId& objectId,
    TreeEntryType treeEntryType,
    const ObjectFetchContextPtr& context) const {
  XLOG(DBG3) << "getTreeEntryForRootId(" << objectId << ")";

  return backingStore_
      ->getTreeEntryForObjectId(objectId, treeEntryType, context)
      .thenValue(
          [](std::shared_ptr<TreeEntry> treeEntry) { return treeEntry; });
}

ImmediateFuture<shared_ptr<const Tree>> ObjectStore::getTree(
    const ObjectId& id,
    const ObjectFetchContextPtr& fetchContext) const {
  DurationScope statScope{stats_, &ObjectStoreStats::getTree};

  // Check in the LocalStore first

  // TODO: We should consider checking if we have in flight BackingStore
  // requests on this layer instead of only in the BackingStore. Consider the
  // case in which thread A and thread B both request a Tree at the same time.
  // Let's say thread A checks the LocalStore, then thread B checks the
  // LocalStore, gets the file from the BackingStore (making a request to the
  // server), then writes the Tree to the LocalStore. Now when thread A checks
  // for in flight requests in the BackingStore, it will not see any since
  // thread B has completely finished, so thread A will make a duplicate
  // request. If we we're to mark here that we got a request on this layer, then
  // we could avoid that case.

  if (auto maybeTree = treeCache_->get(id)) {
    fetchContext->didFetch(
        ObjectFetchContext::Tree, id, ObjectFetchContext::FromMemoryCache);

    updateProcessFetch(*fetchContext);

    return changeCaseSensitivity(maybeTree, caseSensitive_);
  }

  deprioritizeWhenFetchHeavy(*fetchContext);

  return ImmediateFuture{backingStore_->getTree(id, fetchContext)}.thenValue(
      [self = shared_from_this(),
       statScope = std::move(statScope),
       id,
       fetchContext = fetchContext.copy()](BackingStore::GetTreeResult result) {
        if (!result.tree) {
          // TODO: Perhaps we should do some short-term negative
          // caching?
          XLOG(DBG2) << "unable to find tree " << id;
          throwf<std::domain_error>("tree {} not found", id);
        }

        // promote to shared_ptr so we can store in the cache and return
        auto sharedTree = std::shared_ptr<const Tree>(std::move(result.tree));
        self->treeCache_->insert(sharedTree);
        fetchContext->didFetch(ObjectFetchContext::Tree, id, result.origin);
        self->updateProcessFetch(*fetchContext);
        return changeCaseSensitivity(sharedTree, self->caseSensitive_);
      });
}

ImmediateFuture<folly::Unit> ObjectStore::prefetchBlobs(
    ObjectIdRange ids,
    const ObjectFetchContextPtr& fetchContext) const {
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
  return backingStore_->prefetchBlobs(ids, fetchContext);
}

ImmediateFuture<shared_ptr<const Blob>> ObjectStore::getBlob(
    const ObjectId& id,
    const ObjectFetchContextPtr& fetchContext) const {
  DurationScope statScope{stats_, &ObjectStoreStats::getBlob};

  deprioritizeWhenFetchHeavy(*fetchContext);
  return ImmediateFuture<BackingStore::GetBlobResult>{
      backingStore_->getBlob(id, fetchContext)}
      .thenValue(
          [self = shared_from_this(),
           statScope = std::move(statScope),
           id,
           fetchContext =
               fetchContext.copy()](BackingStore::GetBlobResult result)
              -> std::shared_ptr<const Blob> {
            if (!result.blob) {
              // TODO: Perhaps we should do some short-term negative caching?
              XLOG(DBG2) << "unable to find blob " << id;
              throwf<std::domain_error>("blob {} not found", id);
            }
            // Quick check in-memory cache first, before doing expensive
            // calculations. If metadata is present in cache, it most certainly
            // exists in local store too.
            // We always cache metadata in LocalStore because it's faster to
            // query than the BackingStore, and metadata is very small (~28
            // bytes per blob).
            if (!self->metadataCache_.rlock()->exists(id)) {
              auto metadata = computeBlobMetadata(*result.blob);
              self->localStore_->putBlobMetadata(id, metadata);
              self->metadataCache_.wlock()->set(id, metadata);
            }
            self->updateProcessFetch(*fetchContext);
            fetchContext->didFetch(ObjectFetchContext::Blob, id, result.origin);
            return std::move(result.blob);
          });
}

ImmediateFuture<BlobMetadata> ObjectStore::getBlobMetadata(
    const ObjectId& id,
    const ObjectFetchContextPtr& context) const {
  DurationScope statScope{stats_, &ObjectStoreStats::getBlobMetadata};

  // Check in-memory cache
  {
    auto metadataCache = metadataCache_.wlock();
    auto cacheIter = metadataCache->find(id);
    if (cacheIter != metadataCache->end()) {
      stats_->increment(&ObjectStoreStats::getBlobMetadataFromMemory);
      context->didFetch(
          ObjectFetchContext::BlobMetadata,
          id,
          ObjectFetchContext::FromMemoryCache);

      updateProcessFetch(*context);
      return cacheIter->second;
    }
  }

  auto self = shared_from_this();

  // Check local store
  return localStore_->getBlobMetadata(id)
      .thenValue(
          [self,
           statScope = std::move(statScope),
           id,
           context =
               context.copy()](std::optional<BlobMetadata>&& metadata) mutable
          -> ImmediateFuture<BlobMetadata> {
            if (metadata) {
              self->stats_->increment(
                  &ObjectStoreStats::getBlobMetadataFromLocalStore);
              self->metadataCache_.wlock()->set(id, *metadata);
              context->didFetch(
                  ObjectFetchContext::BlobMetadata,
                  id,
                  ObjectFetchContext::FromDiskCache);

              self->updateProcessFetch(*context);
              return *metadata;
            }

            self->deprioritizeWhenFetchHeavy(*context);

            auto localMetadata =
                self->backingStore_->getLocalBlobMetadata(id, context);
            if (localMetadata) {
              self->stats_->increment(
                  &ObjectStoreStats::getLocalBlobMetadataFromBackingStore);
              self->metadataCache_.wlock()->set(id, *localMetadata);
              self->localStore_->putBlobMetadata(id, *localMetadata);
              context->didFetch(
                  ObjectFetchContext::BlobMetadata,
                  id,
                  ObjectFetchContext::FromDiskCache);
              self->updateProcessFetch(*context);
              return *localMetadata;
            }

            // Check backing store
            //
            // TODO: It would be nice to add a smarter API to the BackingStore
            // so that we can query it just for the blob metadata if it supports
            // getting that without retrieving the full blob data.
            //
            // TODO: This should probably check the LocalStore for the blob
            // first, especially when we begin to expire entries in RocksDB.
            return self->backingStore_
                ->getBlob(id, context)
                // Non-blocking statistics and cache updates should happen ASAP
                // rather than waiting for callbacks to be scheduled on the
                // consuming thread.
                .toUnsafeFuture()
                .thenValue([self,
                            statScope = std::move(statScope),
                            id,
                            context = context.copy()](
                               BackingStore::GetBlobResult result) {
                  if (result.blob) {
                    self->stats_->increment(
                        &ObjectStoreStats::getBlobMetadataFromBackingStore);
                    // we retrieved the full blob data
                    self->stats_->increment(
                        &ObjectStoreStats::getBlobFromBackingStore);
                    self->localStore_->putBlob(id, result.blob.get());
                    auto metadata = computeBlobMetadata(*result.blob);
                    self->localStore_->putBlobMetadata(id, metadata);
                    self->metadataCache_.wlock()->set(id, metadata);
                    // I could see an argument for recording this fetch with
                    // type Blob instead of BlobMetadata, but it's probably more
                    // useful in context to know how many metadata fetches
                    // occurred. Also, since backing stores don't directly
                    // support fetching metadata, it should be clear.
                    context->didFetch(
                        ObjectFetchContext::BlobMetadata, id, result.origin);

                    self->updateProcessFetch(*context);
                    return makeFuture(metadata);
                  }

                  throwf<std::domain_error>("blob {} not found", id);
                })
                .semi();
          })
      .semi();
}

ImmediateFuture<uint64_t> ObjectStore::getBlobSize(
    const ObjectId& id,
    const ObjectFetchContextPtr& context) const {
  return getBlobMetadata(id, context)
      .thenValue([](const BlobMetadata& metadata) { return metadata.size; });
}

ImmediateFuture<Hash20> ObjectStore::getBlobSha1(
    const ObjectId& id,
    const ObjectFetchContextPtr& context) const {
  return getBlobMetadata(id, context)
      .thenValue([](const BlobMetadata& metadata) { return metadata.sha1; });
}

ImmediateFuture<bool> ObjectStore::areBlobsEqual(
    const ObjectId& one,
    const ObjectId& two,
    const ObjectFetchContextPtr& context) const {
  if (areObjectsKnownIdentical(one, two)) {
    return true;
  }

  // If Mercurial eventually switches to using blob IDs that are solely
  // based on the file contents (as opposed to file contents + history)
  // then we could drop this extra load of the blob SHA-1, and rely only
  // on the blob ID comparison instead.
  return collectAllSafe(getBlobSha1(one, context), getBlobSha1(two, context))
      .thenValue([](const std::tuple<Hash20, Hash20>& sha1s) {
        return std::get<0>(sha1s) == std::get<1>(sha1s);
      });
}

ObjectComparison ObjectStore::compareObjectsById(
    const ObjectId& one,
    const ObjectId& two) const {
  return backingStore_->compareObjectsById(one, two);
}

bool ObjectStore::areObjectsKnownIdentical(
    const ObjectId& one,
    const ObjectId& two) const {
  return backingStore_->compareObjectsById(one, two) ==
      ObjectComparison::Identical;
}

} // namespace facebook::eden
