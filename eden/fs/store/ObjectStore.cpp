/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/ObjectStore.h"

#include <folly/Conv.h>
#include <folly/futures/Future.h>
#include <folly/io/IOBuf.h>

#include <stdexcept>

#include "eden/common/utils/ProcessInfoCache.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/BackingStore.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/store/TreeCache.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/StructuredLogger.h"
#include "eden/fs/telemetry/TaskTrace.h"
#include "eden/fs/utils/Bug.h"
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
    shared_ptr<BackingStore> backingStore,
    shared_ptr<TreeCache> treeCache,
    EdenStatsPtr stats,
    std::shared_ptr<ProcessInfoCache> processInfoCache,
    std::shared_ptr<StructuredLogger> structuredLogger,
    std::shared_ptr<const EdenConfig> edenConfig,
    bool windowsSymlinksEnabled,
    CaseSensitivity caseSensitive) {
  return std::shared_ptr<ObjectStore>{new ObjectStore{
      std::move(backingStore),
      std::move(treeCache),
      std::move(stats),
      processInfoCache,
      structuredLogger,
      edenConfig,
      windowsSymlinksEnabled,
      caseSensitive}};
}

ObjectStore::ObjectStore(
    shared_ptr<BackingStore> backingStore,
    shared_ptr<TreeCache> treeCache,
    EdenStatsPtr stats,
    std::shared_ptr<ProcessInfoCache> processInfoCache,
    std::shared_ptr<StructuredLogger> structuredLogger,
    std::shared_ptr<const EdenConfig> edenConfig,
    bool windowsSymlinksEnabled,
    CaseSensitivity caseSensitive)
    : metadataCache_{folly::in_place, edenConfig->metadataCacheSize.getValue()},
      treeCache_{std::move(treeCache)},
      backingStore_{std::move(backingStore)},
      stats_{std::move(stats)},
      pidFetchCounts_{std::make_unique<PidFetchCounts>()},
      processInfoCache_(processInfoCache),
      structuredLogger_(structuredLogger),
      edenConfig_(edenConfig),
      caseSensitive_{caseSensitive},
      windowsSymlinksEnabled_{windowsSymlinksEnabled} {
  XCHECK(backingStore_);
  XCHECK(stats_);
}

ObjectStore::~ObjectStore() = default;

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

void ObjectStore::sendFetchHeavyEvent(ProcessId pid, uint64_t fetch_count)
    const {
  auto processName = processInfoCache_->getProcessName(pid.get());
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
  if (auto pid = context.getClientPid()) {
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

} // namespace

ImmediateFuture<ObjectStore::GetRootTreeResult> ObjectStore::getRootTree(
    const RootId& rootId,
    const ObjectFetchContextPtr& context) const {
  XLOG(DBG3) << "getRootTree(" << rootId << ")";
  return backingStore_->getRootTree(rootId, context)
      .thenValue(
          [treeCache = treeCache_, rootId, caseSensitive = caseSensitive_](
              BackingStore::GetRootTreeResult result) {
            treeCache->insert(result.treeId, result.tree);

            return GetRootTreeResult{
                changeCaseSensitivity(std::move(result.tree), caseSensitive),
                result.treeId,
            };
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
  TaskTraceBlock block{"ObjectStore::getTree"};
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
    stats_->increment(&ObjectStoreStats::getTreeFromMemory);
    fetchContext->didFetch(
        ObjectFetchContext::Tree, id, ObjectFetchContext::FromMemoryCache);

    updateProcessFetch(*fetchContext);

    return changeCaseSensitivity(std::move(maybeTree), caseSensitive_);
  }

  deprioritizeWhenFetchHeavy(*fetchContext);

  return ImmediateFuture{backingStore_->getTree(id, fetchContext)}.thenValue(
      [self = shared_from_this(),
       statScope = std::move(statScope),
       id,
       fetchContext = fetchContext.copy()](BackingStore::GetTreeResult result) {
        TaskTraceBlock block2{"ObjectStore::getTree::thenValue"};
        if (!result.tree) {
          // TODO: Perhaps we should do some short-term negative
          // caching?
          XLOG(DBG2) << "unable to find tree " << id;
          throwf<std::domain_error>("tree {} not found", id);
        }

        self->treeCache_->insert(result.tree->getHash(), result.tree);
        fetchContext->didFetch(ObjectFetchContext::Tree, id, result.origin);
        self->updateProcessFetch(*fetchContext);
        return changeCaseSensitivity(
            std::move(result.tree), self->caseSensitive_);
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
            self->updateProcessFetch(*fetchContext);
            fetchContext->didFetch(ObjectFetchContext::Blob, id, result.origin);
            return std::move(result.blob);
          });
}

std::optional<BlobMetadata> ObjectStore::getBlobMetadataFromInMemoryCache(
    const ObjectId& id,
    const ObjectFetchContextPtr& context) const {
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
  return std::nullopt;
}

ImmediateFuture<BlobMetadata> ObjectStore::getBlobMetadata(
    const ObjectId& id,
    const ObjectFetchContextPtr& fetchContext,
    bool blake3Needed) const {
  DurationScope statScope{stats_, &ObjectStoreStats::getBlobMetadata};

  // Check in-memory cache
  auto inMemoryCacheBlobMetadata =
      getBlobMetadataFromInMemoryCache(id, fetchContext);
  if (inMemoryCacheBlobMetadata) {
    if (blake3Needed && !inMemoryCacheBlobMetadata->blake3) {
      return getBlob(id, fetchContext)
          .thenValue(
              [self = shared_from_this(),
               id,
               metadata = std::move(inMemoryCacheBlobMetadata).value()](
                  auto&& blob) mutable -> ImmediateFuture<BlobMetadata> {
                auto blake3 = self->computeBlake3(*blob);
                // updating the metadata with the computed blake3 hash and
                // update the cache
                metadata.blake3.emplace(blake3);
                self->metadataCache_.wlock()->set(id, metadata);
                return metadata;
              });
    }

    return std::move(inMemoryCacheBlobMetadata).value();
  }

  deprioritizeWhenFetchHeavy(*fetchContext);

  return ImmediateFuture<BackingStore::GetBlobMetaResult>{
      backingStore_->getBlobMetadata(id, fetchContext)}
      .thenValue(
          [self = shared_from_this(),
           fetchContext = fetchContext.copy(),
           id,
           statScope = std::move(statScope),
           blake3Needed](BackingStore::GetBlobMetaResult result)
              -> ImmediateFuture<BlobMetadata> {
            if (!result.blobMeta) {
              self->stats_->increment(&ObjectStoreStats::getBlobMetadataFailed);
              XLOG(DBG2) << "unable to find aux data for " << id;
              throwf<std::domain_error>("aux data {} not found", id);
            }

            auto metadata = std::move(result.blobMeta);
            // likely that this case should never happen as backing store should
            // pretty much always always return blake3 but it is better to be
            // extra careful :)
            if (blake3Needed && !metadata->blake3) {
              return self->getBlob(id, fetchContext)
                  .thenValue(
                      [self, id, metadata = std::move(metadata)](
                          auto&& blob) mutable
                      -> ImmediateFuture<BlobMetadata> {
                        auto blake3 = self->computeBlake3(*blob);
                        // updating the metadata with the computed blake3 hash
                        // and update the cache
                        auto metadataCopy = *metadata;
                        metadataCopy.blake3.emplace(blake3);
                        self->metadataCache_.wlock()->set(id, metadataCopy);
                        return metadataCopy;
                      });
            } else {
              self->metadataCache_.wlock()->set(id, *metadata);
              fetchContext->didFetch(
                  ObjectFetchContext::BlobMetadata, id, result.origin);
              self->updateProcessFetch(*fetchContext);
              return *metadata;
            }
          });
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

Hash32 ObjectStore::computeBlake3(const Blob& blob) const {
  const auto content = blob.getContents();
  const auto& maybeBlakeKey = edenConfig_->blake3Key.getValue();
  return maybeBlakeKey ? Hash32::keyedBlake3(
                             folly::ByteRange{folly::StringPiece{
                                 maybeBlakeKey->data(), maybeBlakeKey->size()}},
                             content)
                       : Hash32::blake3(content);
}

ImmediateFuture<Hash32> ObjectStore::getBlobBlake3(
    const ObjectId& id,
    const ObjectFetchContextPtr& context) const {
  return getBlobMetadata(id, context, true /* blake3Needed */)
      .thenValue(
          [id, context = context.copy(), self = shared_from_this()](
              const BlobMetadata& metadata) -> ImmediateFuture<Hash32> {
            if (metadata.blake3) {
              return *metadata.blake3;
            }

            // should never happen but better than crashing
            EDEN_BUG() << fmt::format(
                "Blake3 hash is not defined for id={}", id);
          });
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
  // TODO: replace with blake3 as its faster
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
