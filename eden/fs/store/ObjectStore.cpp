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

#include "eden/common/telemetry/StructuredLogger.h"
#include "eden/common/utils/Bug.h"
#include "eden/common/utils/ImmediateFuture.h"
#include "eden/common/utils/ProcessInfoCache.h"
#include "eden/common/utils/Throw.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeAuxData.h"
#include "eden/fs/store/BackingStore.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/store/TreeCache.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/LogEvent.h"
#include "eden/fs/telemetry/TaskTrace.h"

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
    shared_ptr<LocalStore> localStore,
    shared_ptr<TreeCache> treeCache,
    EdenStatsPtr stats,
    std::shared_ptr<ProcessInfoCache> processInfoCache,
    std::shared_ptr<StructuredLogger> structuredLogger,
    std::shared_ptr<ReloadableConfig> edenConfig,
    bool windowsSymlinksEnabled,
    CaseSensitivity caseSensitive) {
  return std::shared_ptr<ObjectStore>{new ObjectStore{
      std::move(backingStore),
      std::move(localStore),
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
    shared_ptr<LocalStore> localStore,
    shared_ptr<TreeCache> treeCache,
    EdenStatsPtr stats,
    std::shared_ptr<ProcessInfoCache> processInfoCache,
    std::shared_ptr<StructuredLogger> structuredLogger,
    std::shared_ptr<ReloadableConfig> edenConfig,
    bool windowsSymlinksEnabled,
    CaseSensitivity caseSensitive)
    : blobAuxDataCache_{
          edenConfig->getEdenConfig()->metadataCacheShards.getValue(),
          edenConfig->getEdenConfig()->metadataCacheSize.getValue()},
      treeAuxDataCache_{
          edenConfig->getEdenConfig()->metadataCacheShards.getValue(),
          edenConfig->getEdenConfig()->metadataCacheSize.getValue()},
      treeCache_{std::move(treeCache)},
      backingStore_{std::move(backingStore)},
      localStore_{std::move(localStore)},
      localStoreCachingPolicy_{backingStore_->getLocalStoreCachingPolicy()},
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
    auto threshold =
        edenConfig_->getEdenConfig()->fetchHeavyThreshold.getValue();
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
    XLOGF(
        WARN,
        "Heavy fetches ({}) from process {}(pid={})",
        fetch_count,
        *processName,
        pid);
    auto repoName = backingStore_->getRepoName();
    std::optional<uint64_t> loadedInodes = [repoName]() {
      auto counterValue = fb303::ServiceData::get()->getCounterIfExists(
          fmt::format("inodemap.{}.loaded", repoName.value_or("")));
      return counterValue.has_value()
          ? std::optional<uint64_t>(static_cast<uint64_t>(counterValue.value()))
          : std::nullopt;
    }();

    structuredLogger_->logEvent(
        FetchHeavy{processName.value(), pid, fetch_count, loadedInodes});
  } else {
    XLOGF(WARN, "Heavy fetches ({}) from pid {})", fetch_count, pid);
  }
}

void ObjectStore::deprioritizeWhenFetchHeavy(
    ObjectFetchContext& context) const {
  if (auto pid = context.getClientPid()) {
    auto fetch_count = pidFetchCounts_->getCountByPid(pid.value());
    auto threshold =
        edenConfig_->getEdenConfig()->fetchHeavyThreshold.getValue();
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
        std::move(treeEntries), tree->getObjectId());
  }
}

} // namespace

ImmediateFuture<ObjectStore::GetRootTreeResult> ObjectStore::getRootTree(
    const RootId& rootId,
    const ObjectFetchContextPtr& context) const {
  XLOGF(DBG3, "getRootTree({})", rootId);
  DurationScope<EdenStats> statScope{stats_, &ObjectStoreStats::getRootTree};

  // TODO: this code caches the root tree, but doesn't have similar code to look
  // it up This should be investigated and either changed or the reasoning
  // should be documented
  return backingStore_->getRootTree(rootId, context)
      .thenValue([self = shared_from_this(), localStore = localStore_, rootId](
                     BackingStore::GetRootTreeResult result) {
        self->stats_->increment(&ObjectStoreStats::getRootTreeFromBackingStore);
        if (self->shouldCacheOnDisk(
                BackingStore::LocalStoreCachingPolicy::Anything)) {
          // TODO: perhaps this callback should use
          // via(&InlineExecutor::instance()) to ensure the tree is cached
          // whether or not the caller consumes the future. See comments on
          // D39475223. Also, before changing, see eden/fs/docs/Futures.md
          // for more details on possible pitfalls of InlineExecutor.
          localStore->putTree(*result.tree);
        }
        return result;
      })
      .thenValue(
          [treeCache = treeCache_, rootId, caseSensitive = caseSensitive_](
              BackingStore::GetRootTreeResult result) {
            auto tree =
                changeCaseSensitivity(std::move(result.tree), caseSensitive);
            treeCache->insert(result.treeId, tree);

            return GetRootTreeResult{
                std::move(tree),
                result.treeId,
            };
          })
      .thenError(
          [this, rootId](const folly::exception_wrapper& ew)
              -> ImmediateFuture<ObjectStore::GetRootTreeResult> {
            stats_->increment(&ObjectStoreStats::getRootTreeFailed);
            XLOGF(DBG4, "unable to find root tree {}", rootId.value());
            return makeImmediateFuture<ObjectStore::GetRootTreeResult>(ew);
          })
      .ensure([scope = std::move(statScope)] {});
}

ImmediateFuture<std::shared_ptr<TreeEntry>>
ObjectStore::getTreeEntryForObjectId(
    const ObjectId& objectId,
    TreeEntryType treeEntryType,
    const ObjectFetchContextPtr& context) const {
  XLOGF(DBG3, "getTreeEntryForRootId({})", objectId);

  return backingStore_
      ->getTreeEntryForObjectId(objectId, treeEntryType, context)
      .thenValue(
          [](std::shared_ptr<TreeEntry> treeEntry) { return treeEntry; });
}

ImmediateFuture<shared_ptr<const Tree>> ObjectStore::getTree(
    const ObjectId& id,
    const ObjectFetchContextPtr& fetchContext) const {
  TaskTraceBlock block{"ObjectStore::getTree"};
  DurationScope<EdenStats> statScope{stats_, &ObjectStoreStats::getTree};
  folly::stop_watch<std::chrono::milliseconds> watch;

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
    stats_->addDuration(
        &ObjectStoreStats::getTreeMemoryDuration, watch.elapsed());
    return changeCaseSensitivity(std::move(maybeTree), caseSensitive_);
  }

  deprioritizeWhenFetchHeavy(*fetchContext);

  return ImmediateFuture{getTreeImpl(id, fetchContext, watch)}.thenValue(
      [self = shared_from_this(),
       statScope = std::move(statScope),
       id,
       fetchContext = fetchContext.copy()](BackingStore::GetTreeResult result) {
        TaskTraceBlock block2{"ObjectStore::getTree::thenValue"};
        auto tree =
            changeCaseSensitivity(std::move(result.tree), self->caseSensitive_);
        self->treeCache_->insert(tree->getObjectId(), tree);
        fetchContext->didFetch(ObjectFetchContext::Tree, id, result.origin);
        self->updateProcessFetch(*fetchContext);
        return tree;
      });
}

void ObjectStore::maybeCacheTreeAndAuxInLocalStore(
    const ObjectId& id,
    const BackingStore::GetTreeResult& treeResult) const {
  auto shouldCacheTree =
      shouldCacheOnDisk(BackingStore::LocalStoreCachingPolicy::Trees);
  auto shouldCacheBlobAuxData =
      shouldCacheOnDisk(BackingStore::LocalStoreCachingPolicy::BlobAuxData);
  auto shouldCacheTreeAuxData =
      shouldCacheOnDisk(BackingStore::LocalStoreCachingPolicy::TreeAuxData);

  if (!shouldCacheTree && !shouldCacheBlobAuxData && !shouldCacheTreeAuxData) {
    return;
  }

  auto batch = localStore_->beginWrite();
  if (shouldCacheTree) {
    batch->putTree(*treeResult.tree);
  }

  if (shouldCacheBlobAuxData) {
    // Let's cache all the entries in the LocalStore.
    for (const auto& [name, treeEntry] : *treeResult.tree) {
      const auto& size = treeEntry.getSize();
      const auto& sha1 = treeEntry.getContentSha1();
      const auto& blake3 = treeEntry.getContentBlake3();
      if (treeEntry.getType() == TreeEntryType::REGULAR_FILE && size && sha1) {
        batch->putBlobAuxData(
            treeEntry.getObjectId(), BlobAuxData{*sha1, blake3, *size});
      }
    }
  }

  // Pre-warm the cache if there is tree aux data available
  if (treeResult.tree && treeResult.tree->getAuxData()) {
    if (shouldCacheTreeAuxData &&
        edenConfig_->getEdenConfig()
            ->warmTreeAuxLocalCacheIfTreeFromBackingStore.getValue()) {
      stats_->increment(
          &ObjectStoreStats::prewarmTreeAuxLocalCacheForTreeFromBackingStore);
      localStore_->putTreeAuxData(id, *treeResult.tree->getAuxData());
    }
  }

  batch->flush();
}

void ObjectStore::maybeCacheTreeAuxInMemCache(
    const ObjectId& id,
    const BackingStore::GetTreeResult& treeResult) const {
  if (treeResult.tree && treeResult.tree->getAuxData() &&
      edenConfig_->getEdenConfig()
          ->warmTreeAuxMemCacheIfTreeFromBackingStore.getValue()) {
    stats_->increment(
        &ObjectStoreStats::prewarmTreeAuxMemCacheForTreeFromBackingStore);
    treeAuxDataCache_.store(id, *treeResult.tree->getAuxData());
  }
}

folly::SemiFuture<BackingStore::GetTreeResult> ObjectStore::getTreeImpl(
    const ObjectId& id,
    const ObjectFetchContextPtr& context,
    folly::stop_watch<std::chrono::milliseconds> watch) const {
  auto localStoreGetTree = ImmediateFuture<TreePtr>{std::in_place, nullptr};
  if (shouldCacheOnDisk(BackingStore::LocalStoreCachingPolicy::Trees)) {
    // Check Local Store first
    localStoreGetTree = localStore_->getTree(id);
  }
  return std::move(localStoreGetTree)
      .thenValue(
          [self = shared_from_this(), id = id, context = context.copy(), watch](
              TreePtr tree) mutable
          -> ImmediateFuture<BackingStore::GetTreeResult> {
            if (tree) {
              self->stats_->increment(&ObjectStoreStats::getTreeFromLocalStore);
              self->stats_->addDuration(
                  &ObjectStoreStats::getTreeLocalstoreDuration,
                  watch.elapsed());
              if (tree->getAuxData() == nullptr &&
                  self->edenConfig_->getEdenConfig()
                      ->warmTreeAuxCacheIfTreeFromLocalStore.getValue()) {
                // The tree stored locally does not have Tree Aux Data attached.
                // This means the Tree was fetched before Tree serialization V2
                // and before we support TreeAuxData included in Tree response
                // from SaplingBackingStore. Otherwise, we would have the Tree
                // Aux Data pre-cached already.
                // So, it's very likely that we don't have Tree Aux Data cached.
                // Triggering a getTreeAuxData request to warm the cache
                self->stats_->increment(
                    &ObjectStoreStats::
                        prewarmTreeAuxCacheForTreeFetchedFromLocalStore);
                return self->getTreeAuxData(id, context)
                    .thenTry([self, tree = tree, id = id](
                                 folly::Try<std::optional<TreeAuxData>>&&
                                     auxResult) {
                      if (auxResult.hasException() ||
                          !auxResult.value().has_value()) {
                        self->stats_->increment(
                            &ObjectStoreStats::
                                prewarmTreeAuxCacheForTreeFetchedFromLocalStoreFailed);
                        XLOGF(
                            DBG5,
                            "Failed to fetch Tree Aux Data for Tree {}: {}",
                            id,
                            auxResult.hasException()
                                ? auxResult.exception().what()
                                : "no tree aux data");
                      }
                      return BackingStore::GetTreeResult{
                          tree, ObjectFetchContext::FromDiskCache};
                    });
              }
              // Maybe there is Tree Aux Data attached to this Tree?
              // If that' the case, we should've already populated
              // Tree aux data into the local cache when the tree was
              // fetched from sapling backing store.
              // So we should not bother warming it into the in-memory
              // cache since that requires a write lock.
              return BackingStore::GetTreeResult{
                  std::move(tree), ObjectFetchContext::FromDiskCache};
            }

            return ImmediateFuture{self->backingStore_->getTree(id, context)}
                // TODO: This is a good use for via(&InlineExecutor::instance())
                // to ensure the tree is cached even if the resulting future is
                // never consumed. Before changing, see eden/fs/docs/Futures.md
                // for more details on possible pitfalls of InlineExecutor.
                .thenValue(
                    [self, id, watch](BackingStore::GetTreeResult result) {
                      self->maybeCacheTreeAndAuxInLocalStore(id, result);
                      self->maybeCacheTreeAuxInMemCache(id, result);
                      self->stats_->increment(
                          &ObjectStoreStats::getTreeFromBackingStore);
                      self->stats_->addDuration(
                          &ObjectStoreStats::getTreeBackingstoreDuration,
                          watch.elapsed());
                      return result;
                    })
                .thenError(
                    [self, id](const folly::exception_wrapper& ew)
                        -> ImmediateFuture<BackingStore::GetTreeResult> {
                      self->stats_->increment(&ObjectStoreStats::getTreeFailed);
                      XLOGF(DBG4, "unable to find tree {}", id);
                      return makeImmediateFuture<BackingStore::GetTreeResult>(
                          ew);
                    });
          })
      .semi();
}

ImmediateFuture<std::optional<TreeAuxData>> ObjectStore::getTreeAuxData(
    const ObjectId& id,
    const ObjectFetchContextPtr& fetchContext) const {
  DurationScope<EdenStats> statScope{stats_, &ObjectStoreStats::getTreeAuxData};
  folly::stop_watch<std::chrono::milliseconds> watch;

  // Check in-memory cache
  auto inMemoryCacheTreeAuxData =
      getTreeAuxDataFromInMemoryCache(id, fetchContext);
  if (inMemoryCacheTreeAuxData) {
    stats_->increment(&ObjectStoreStats::getTreeAuxDataFromMemory);
    stats_->addDuration(
        &ObjectStoreStats::getTreeAuxDataMemoryDuration, watch.elapsed());
    return std::move(inMemoryCacheTreeAuxData).value();
  }

  deprioritizeWhenFetchHeavy(*fetchContext);

  return ImmediateFuture<BackingStore::GetTreeAuxResult>{
      getTreeAuxDataImpl(id, fetchContext, watch)}
      .thenValue(
          [self = shared_from_this(),
           fetchContext = fetchContext.copy(),
           id,
           statScope =
               std::move(statScope)](BackingStore::GetTreeAuxResult result)
              -> ImmediateFuture<std::optional<TreeAuxData>> {
            if (!result.treeAux) {
              self->stats_->increment(&ObjectStoreStats::getTreeAuxDataFailed);
              XLOGF(DBG4, "unable to find aux data for {}", id);
              return std::nullopt;
            }
            auto auxData = std::move(result.treeAux);
            self->treeAuxDataCache_.store(id, *auxData);
            fetchContext->didFetch(
                ObjectFetchContext::TreeAuxData, id, result.origin);
            self->updateProcessFetch(*fetchContext);
            return *auxData;
          });
}

folly::SemiFuture<BackingStore::GetTreeAuxResult>
ObjectStore::getTreeAuxDataImpl(
    const ObjectId& id,
    const ObjectFetchContextPtr& context,
    folly::stop_watch<std::chrono::milliseconds> watch) const {
  auto localStoreGetTreeAuxData =
      ImmediateFuture<TreeAuxDataPtr>{std::in_place, nullptr};
  if (shouldCacheOnDisk(BackingStore::LocalStoreCachingPolicy::TreeAuxData)) {
    localStoreGetTreeAuxData = localStore_->getTreeAuxData(id);
  }

  return std::move(localStoreGetTreeAuxData)
      .thenValue(
          [self = shared_from_this(), id = id, context = context.copy(), watch](
              TreeAuxDataPtr auxData) mutable
          -> ImmediateFuture<BackingStore::GetTreeAuxResult> {
            if (auxData) {
              self->stats_->increment(
                  &ObjectStoreStats::getTreeAuxDataFromLocalStore);
              self->stats_->addDuration(
                  &ObjectStoreStats::getTreeAuxDataLocalstoreDuration,
                  watch.elapsed());
              return BackingStore::GetTreeAuxResult{
                  std::move(auxData), ObjectFetchContext::FromDiskCache};
            }

            return ImmediateFuture{
                self->backingStore_->getTreeAuxData(id, context)}
                .thenValue(
                    [self, id, context = context.copy(), watch](
                        BackingStore::GetTreeAuxResult result)
                        -> ImmediateFuture<BackingStore::GetTreeAuxResult> {
                      if (result.treeAux) {
                        self->stats_->increment(
                            &ObjectStoreStats::getTreeAuxDataFromBackingStore);
                        self->stats_->addDuration(
                            &ObjectStoreStats::
                                getTreeAuxDataBackingstoreDuration,
                            watch.elapsed());
                        return result;
                      }
                      self->stats_->increment(
                          &ObjectStoreStats::getTreeAuxDataFailed);
                      return BackingStore::GetTreeAuxResult{
                          nullptr, ObjectFetchContext::Origin::NotFetched};
                    })
                .thenValue([self, id](BackingStore::GetTreeAuxResult result) {
                  if (result.treeAux &&
                      self->shouldCacheOnDisk(
                          BackingStore::LocalStoreCachingPolicy::TreeAuxData)) {
                    self->localStore_->putTreeAuxData(id, *result.treeAux);
                  }
                  return result;
                })
                .thenError(
                    [self, id](const folly::exception_wrapper& ew)
                        -> ImmediateFuture<BackingStore::GetTreeAuxResult> {
                      self->stats_->increment(
                          &ObjectStoreStats::getTreeAuxDataFailed);
                      XLOGF(DBG4, "unable to find aux data for {}", id);
                      return makeImmediateFuture<
                          BackingStore::GetTreeAuxResult>(ew);
                    });
          })
      .semi();
}

ImmediateFuture<std::optional<Hash32>> ObjectStore::getTreeDigestHash(
    const ObjectId& id,
    const ObjectFetchContextPtr& context) const {
  return getTreeAuxData(id, context)
      .thenValue(
          [id, context = context.copy(), self = shared_from_this()](
              const std::optional<TreeAuxData>& auxData)
              -> ImmediateFuture<std::optional<Hash32>> {
            return auxData.has_value() ? auxData->digestHash : std::nullopt;
          });
}

ImmediateFuture<std::optional<uint64_t>> ObjectStore::getTreeDigestSize(
    const ObjectId& id,
    const ObjectFetchContextPtr& context) const {
  return getTreeAuxData(id, context)
      .thenValue([](const std::optional<TreeAuxData>& auxData) {
        return auxData.has_value()
            ? std::optional<uint64_t>(auxData->digestSize)
            : std::nullopt;
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
  // sapling backing store to ensure that its local cache storage
  // has entries for all of the requested keys.
  if (ids.empty()) {
    return folly::unit;
  }
  return backingStore_->prefetchBlobs(ids, fetchContext);
}

ImmediateFuture<shared_ptr<const Blob>> ObjectStore::getBlob(
    const ObjectId& id,
    const ObjectFetchContextPtr& fetchContext) const {
  DurationScope<EdenStats> statScope{stats_, &ObjectStoreStats::getBlob};

  deprioritizeWhenFetchHeavy(*fetchContext);

  return ImmediateFuture<BackingStore::GetBlobResult>{
      getBlobImpl(id, fetchContext)}
      .thenValue(
          [self = shared_from_this(),
           statScope = std::move(statScope),
           id,
           fetchContext =
               fetchContext.copy()](BackingStore::GetBlobResult result)
              -> std::shared_ptr<const Blob> {
            self->updateProcessFetch(*fetchContext);
            fetchContext->didFetch(ObjectFetchContext::Blob, id, result.origin);
            return std::move(result.blob);
          });
}

folly::SemiFuture<BackingStore::GetBlobResult> ObjectStore::getBlobImpl(
    const ObjectId& id,
    const ObjectFetchContextPtr& context) const {
  auto localStoreGetBlob = ImmediateFuture<BlobPtr>{std::in_place, nullptr};
  if (shouldCacheOnDisk(BackingStore::LocalStoreCachingPolicy::Blobs)) {
    // Check in the LocalStore first
    localStoreGetBlob = localStore_->getBlob(id);
  }
  return std::move(localStoreGetBlob)
      .thenValue(
          [self = shared_from_this(), id = id, context = context.copy()](
              BlobPtr blob) mutable
          -> ImmediateFuture<BackingStore::GetBlobResult> {
            if (blob) {
              self->stats_->increment(&ObjectStoreStats::getBlobFromLocalStore);
              return BackingStore::GetBlobResult{
                  std::move(blob), ObjectFetchContext::FromDiskCache};
            }

            // If we didn't find the blob in the LocalStore, then fetch it
            // from the BackingStore.
            return ImmediateFuture{self->backingStore_->getBlob(id, context)}
                // TODO: This is a good use for via(&InlineExecutor::instance())
                // to ensure the blob is cached even if the resulting future is
                // never consumed. Before changing, see eden/fs/docs/Futures.md
                // for more details on possible pitfalls of InlineExecutor.
                .thenValue([self, id](BackingStore::GetBlobResult result) {
                  if (self->shouldCacheOnDisk(
                          BackingStore::LocalStoreCachingPolicy::Blobs)) {
                    self->localStore_->putBlob(id, result.blob.get());
                  }
                  self->stats_->increment(
                      &ObjectStoreStats::getBlobFromBackingStore);
                  return result;
                })
                .thenError(
                    [self, id](const folly::exception_wrapper& ew)
                        -> ImmediateFuture<BackingStore::GetBlobResult> {
                      self->stats_->increment(&ObjectStoreStats::getBlobFailed);
                      XLOGF(DBG4, "unable to find blob {}", id);
                      return makeImmediateFuture<BackingStore::GetBlobResult>(
                          ew);
                    });
          })
      .semi();
}

std::optional<BlobAuxData> ObjectStore::getBlobAuxDataFromInMemoryCache(
    const ObjectId& id,
    const ObjectFetchContextPtr& context) const {
  auto ret = blobAuxDataCache_.get(id);
  if (ret) {
    context->didFetch(
        ObjectFetchContext::BlobAuxData,
        id,
        ObjectFetchContext::FromMemoryCache);

    updateProcessFetch(*context);
  }

  return ret;
}

// TODO: This code is "identical" to the blob code. Though it is small today, it
// might make sense to refactor into some kind of template and/or helper
// methods.
std::optional<TreeAuxData> ObjectStore::getTreeAuxDataFromInMemoryCache(
    const ObjectId& id,
    const ObjectFetchContextPtr& context) const {
  auto ret = treeAuxDataCache_.get(id);
  if (ret) {
    context->didFetch(
        ObjectFetchContext::TreeAuxData,
        id,
        ObjectFetchContext::FromMemoryCache);

    updateProcessFetch(*context);
  }

  return ret;
}

ImmediateFuture<BlobAuxData> ObjectStore::getBlobAuxData(
    const ObjectId& id,
    const ObjectFetchContextPtr& fetchContext,
    bool blake3Needed) const {
  DurationScope<EdenStats> statScope{stats_, &ObjectStoreStats::getBlobAuxData};
  folly::stop_watch<std::chrono::milliseconds> watch;

  // Check in-memory cache
  auto inMemoryCacheBlobAuxData =
      getBlobAuxDataFromInMemoryCache(id, fetchContext);
  if (inMemoryCacheBlobAuxData) {
    if (blake3Needed && !inMemoryCacheBlobAuxData->blake3) {
      return getBlob(id, fetchContext)
          .thenValue(
              [self = shared_from_this(),
               id,
               auxData = std::move(inMemoryCacheBlobAuxData).value(),
               watch](auto&& blob) mutable -> ImmediateFuture<BlobAuxData> {
                auto blake3 = self->computeBlake3(*blob);
                // updating the aux data with the computed blake3 hash and
                // update the cache
                auxData.blake3.emplace(blake3);
                self->blobAuxDataCache_.store(id, auxData);
                self->stats_->increment(
                    &ObjectStoreStats::getBlobAuxDataFromBlob);
                self->stats_->addDuration(
                    &ObjectStoreStats::getBlobAuxDataFromBlobDuration,
                    watch.elapsed());
                return auxData;
              });
    }
    stats_->increment(&ObjectStoreStats::getBlobAuxDataFromMemory);
    stats_->addDuration(
        &ObjectStoreStats::getBlobAuxDataMemoryDuration, watch.elapsed());
    return std::move(inMemoryCacheBlobAuxData).value();
  }

  deprioritizeWhenFetchHeavy(*fetchContext);

  return ImmediateFuture<BackingStore::GetBlobAuxResult>{
      getBlobAuxDataImpl(id, fetchContext, watch)}
      .thenValue(
          [self = shared_from_this(),
           fetchContext = fetchContext.copy(),
           id,
           statScope = std::move(statScope),
           blake3Needed](BackingStore::GetBlobAuxResult result)
              -> ImmediateFuture<BlobAuxData> {
            if (!result.blobAux) {
              self->stats_->increment(&ObjectStoreStats::getBlobAuxDataFailed);
              XLOGF(DBG4, "unable to find aux data for {}", id);
              throwf<std::domain_error>("aux data {} not found", id);
            }
            auto auxData = std::move(result.blobAux);
            // likely that this case should never happen as backing store should
            // pretty much always always return blake3 but it is better to be
            // extra careful :)
            if (blake3Needed && !auxData->blake3) {
              return self->getBlob(id, fetchContext)
                  .thenValue(
                      [self, id, auxData = std::move(auxData)](
                          auto&& blob) mutable -> ImmediateFuture<BlobAuxData> {
                        auto blake3 = self->computeBlake3(*blob);
                        // updating the aux data with the computed blake3 hash
                        // and update the cache
                        auto auxDataCopy = *auxData;
                        auxDataCopy.blake3.emplace(blake3);
                        self->blobAuxDataCache_.store(id, auxDataCopy);
                        return auxDataCopy;
                      });
            } else {
              self->blobAuxDataCache_.store(id, *auxData);
              fetchContext->didFetch(
                  ObjectFetchContext::BlobAuxData, id, result.origin);
              self->updateProcessFetch(*fetchContext);
              return *auxData;
            }
          });
}

folly::SemiFuture<BackingStore::GetBlobAuxResult>
ObjectStore::getBlobAuxDataImpl(
    const ObjectId& id,
    const ObjectFetchContextPtr& context,
    folly::stop_watch<std::chrono::milliseconds> watch) const {
  auto localStoreGetBlobAuxData =
      ImmediateFuture<BlobAuxDataPtr>{std::in_place, nullptr};
  if (shouldCacheOnDisk(BackingStore::LocalStoreCachingPolicy::BlobAuxData)) {
    localStoreGetBlobAuxData = localStore_->getBlobAuxData(id);
  }

  return std::move(localStoreGetBlobAuxData)
      .thenValue(
          [self = shared_from_this(), id = id, context = context.copy(), watch](
              BlobAuxDataPtr auxData) mutable
          -> ImmediateFuture<BackingStore::GetBlobAuxResult> {
            if (auxData) {
              self->stats_->increment(
                  &ObjectStoreStats::getBlobAuxDataFromLocalStore);
              self->stats_->addDuration(
                  &ObjectStoreStats::getBlobAuxDataLocalstoreDuration,
                  watch.elapsed());
              return BackingStore::GetBlobAuxResult{
                  std::move(auxData), ObjectFetchContext::FromDiskCache};
            }

            return ImmediateFuture{
                self->backingStore_->getBlobAuxData(id, context)}
                .thenValue(
                    [self, id, context = context.copy(), watch](
                        BackingStore::GetBlobAuxResult result)
                        -> ImmediateFuture<BackingStore::GetBlobAuxResult> {
                      if (result.blobAux &&
                          result.blobAux->sha1 !=
                              kZeroHash) { // from eden/fs/model/Hash.cpp
                        self->stats_->increment(
                            &ObjectStoreStats::getBlobAuxDataFromBackingStore);
                        self->stats_->addDuration(
                            &ObjectStoreStats::
                                getBlobAuxDataBackingstoreDuration,
                            watch.elapsed());
                        return result;
                      }

                      return ImmediateFuture{self->getBlobImpl(id, context)}
                          .thenValue([self,
                                      backingStoreResult = std::move(result),
                                      watch](
                                         BackingStore::GetBlobResult result) {
                            if (result.blob) {
                              self->stats_->increment(
                                  &ObjectStoreStats::getBlobAuxDataFromBlob);

                              std::optional<Hash32> blake3;
                              if (backingStoreResult.blobAux &&
                                  backingStoreResult.blobAux->blake3
                                      .has_value()) {
                                blake3 =
                                    backingStoreResult.blobAux->blake3.value();
                              }

                              self->stats_->addDuration(
                                  &ObjectStoreStats::
                                      getBlobAuxDataFromBlobDuration,
                                  watch.elapsed());

                              return BackingStore::GetBlobAuxResult{
                                  std::make_shared<BlobAuxData>(
                                      Hash20::sha1(result.blob->getContents()),
                                      std::move(blake3),
                                      result.blob->getSize()),
                                  result.origin};
                            }
                            self->stats_->increment(
                                &ObjectStoreStats::getBlobAuxDataFailed);
                            return BackingStore::GetBlobAuxResult{
                                nullptr,
                                ObjectFetchContext::Origin::NotFetched};
                          });
                    })
                .thenValue([self, id](BackingStore::GetBlobAuxResult result) {
                  if (result.blobAux &&
                      self->shouldCacheOnDisk(
                          BackingStore::LocalStoreCachingPolicy::BlobAuxData)) {
                    self->localStore_->putBlobAuxData(id, *result.blobAux);
                  }
                  return result;
                })
                .thenError(
                    [self, id](const folly::exception_wrapper& ew)
                        -> ImmediateFuture<BackingStore::GetBlobAuxResult> {
                      self->stats_->increment(
                          &ObjectStoreStats::getBlobAuxDataFailed);
                      XLOGF(DBG4, "unable to find aux data for {}", id);
                      return makeImmediateFuture<
                          BackingStore::GetBlobAuxResult>(ew);
                    });
          })
      .semi();
}

ImmediateFuture<uint64_t> ObjectStore::getBlobSize(
    const ObjectId& id,
    const ObjectFetchContextPtr& context) const {
  return getBlobAuxData(id, context).thenValue([](const BlobAuxData& auxData) {
    return auxData.size;
  });
}

ImmediateFuture<Hash20> ObjectStore::getBlobSha1(
    const ObjectId& id,
    const ObjectFetchContextPtr& context) const {
  return getBlobAuxData(id, context).thenValue([](const BlobAuxData& auxData) {
    return auxData.sha1;
  });
}

Hash32 ObjectStore::computeBlake3(const Blob& blob) const {
  const auto content = blob.getContents();
  // This should maybe be read at startup and saved in a member variable, but in
  // practice this key should never change.
  const auto& maybeBlakeKey =
      edenConfig_->getEdenConfig()->blake3Key.getValue();
  return maybeBlakeKey ? Hash32::keyedBlake3(
                             folly::ByteRange{folly::StringPiece{
                                 maybeBlakeKey->data(), maybeBlakeKey->size()}},
                             content)
                       : Hash32::blake3(content);
}

ImmediateFuture<Hash32> ObjectStore::getBlobBlake3(
    const ObjectId& id,
    const ObjectFetchContextPtr& context) const {
  return getBlobAuxData(id, context, true /* blake3Needed */)
      .thenValue(
          [id, context = context.copy(), self = shared_from_this()](
              const BlobAuxData& auxData) -> ImmediateFuture<Hash32> {
            if (auxData.blake3) {
              return *auxData.blake3;
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

ImmediateFuture<BackingStore::GetGlobFilesResult> ObjectStore::getGlobFiles(
    const RootId& id,
    const std::vector<std::string>& globs,
    const std::vector<std::string>& prefixes,
    const ObjectFetchContextPtr& context) const {
  return getGlobFilesImpl(id, globs, prefixes, context);
}

ImmediateFuture<BackingStore::GetGlobFilesResult> ObjectStore::getGlobFilesImpl(
    const RootId& id,
    const std::vector<std::string>& globs,
    const std::vector<std::string>& prefixes,
    const ObjectFetchContextPtr& /*context*/) const {
  return backingStore_->getGlobFiles(id, globs, prefixes);
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

bool ObjectStore::shouldCacheOnDisk(
    BackingStore::LocalStoreCachingPolicy object) const {
  return (
      folly::to_underlying(localStoreCachingPolicy_) &
      folly::to_underlying(object));
}

} // namespace facebook::eden
