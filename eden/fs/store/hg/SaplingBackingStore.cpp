/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/hg/SaplingBackingStore.h"

#include <algorithm>
#include <chrono>
#include <thread>
#include <utility>
#include <variant>

#include <re2/re2.h>

#include <folly/Executor.h>
#include <folly/Range.h>
#include <folly/String.h>
#include <folly/executors/CPUThreadPoolExecutor.h>
#include <folly/executors/task_queue/UnboundedBlockingQueue.h>
#include <folly/executors/thread_factory/InitThreadFactory.h>
#include <folly/futures/Future.h>
#include <folly/logging/xlog.h>
#include <folly/system/ThreadName.h>
#include <gflags/gflags.h>

#include "eden/common/telemetry/RequestMetricsScope.h"
#include "eden/common/telemetry/StructuredLogger.h"
#include "eden/common/utils/Bug.h"
#include "eden/common/utils/EnumValue.h"
#include "eden/common/utils/FaultInjector.h"
#include "eden/common/utils/PathFuncs.h"
#include "eden/common/utils/Throw.h"
#include "eden/common/utils/UnboundedQueueExecutor.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/BlobAuxData.h"
#include "eden/fs/model/TreeAuxData.h"
#include "eden/fs/service/ThriftUtil.h"
#include "eden/fs/store/BackingStoreLogger.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/store/StoreResult.h"
#include "eden/fs/store/hg/HgProxyHash.h"
#include "eden/fs/store/hg/SaplingImportRequest.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/LogEvent.h"
#include "eden/fs/utils/StaticAssert.h"
#ifdef EDEN_HAVE_SERVER_OBSERVER
#include "common/fb303/cpp/ThreadPoolExecutorCounters.h" // @manual
#endif
#include "common/network/Hostname.h"

DEFINE_bool(
    hg_fetch_missing_trees,
    true,
    "Set this parameter to \"no\" to disable fetching missing treemanifest "
    "trees from the remote mercurial server.  This is generally only useful "
    "for testing/debugging purposes");

namespace facebook::eden {

namespace {
// 100,000 hg object fetches in a short term is plausible.
constexpr size_t kTraceBusCapacity = 100000;
static_assert(CheckSize<HgImportTraceEvent, 72>());
// TraceBus is double-buffered, so the following capacity should be doubled.
// 10 MB overhead per backing repo is tolerable.
static_assert(
    CheckEqual<7200000, kTraceBusCapacity * sizeof(HgImportTraceEvent)>());
ObjectId hashFromRootId(const RootId& root) {
  return ObjectId::fromHex(root.value());
}

std::unique_ptr<SaplingBackingStoreOptions> computeRuntimeOptions(
    std::unique_ptr<SaplingBackingStoreOptions> options) {
  // No options are currently set. See D64436672 for an example on how to add
  // this back if the mechanism is needed in the future.
  return options;
}

TreeEntryType fromRawTreeEntryType(sapling::TreeEntryType type) {
  switch (type) {
    case sapling::TreeEntryType::RegularFile:
      return TreeEntryType::REGULAR_FILE;
    case sapling::TreeEntryType::Tree:
      return TreeEntryType::TREE;
    case sapling::TreeEntryType::ExecutableFile:
      return TreeEntryType::EXECUTABLE_FILE;
    case sapling::TreeEntryType::Symlink:
      return TreeEntryType::SYMLINK;
  }
  EDEN_BUG() << "unknown tree entry type " << static_cast<uint32_t>(type)
             << " loaded from data store";
}

Tree::value_type fromRawTreeEntry(
    sapling::TreeEntry entry,
    RelativePathPiece path,
    HgObjectIdFormat hgObjectIdFormat) {
  std::optional<uint64_t> size;
  std::optional<Hash20> contentSha1;
  std::optional<Hash32> contentBlake3;

  if (entry.has_size) {
    size = entry.size;
  }

  if (entry.has_sha1) {
    contentSha1.emplace(Hash20(std::move(entry.content_sha1)));
  }

  if (entry.has_blake3) {
    contentBlake3.emplace(Hash32(std::move(entry.content_blake3)));
  }

  auto name = PathComponent(folly::StringPiece{
      folly::ByteRange{entry.name.data(), entry.name.size()}});
  Hash20 hash(std::move(entry.hash));

  auto fullPath = path + name;
  auto proxyHash = HgProxyHash::store(fullPath, hash, hgObjectIdFormat);

  auto treeEntry = TreeEntry{
      proxyHash,
      fromRawTreeEntryType(entry.ttype),
      size,
      contentSha1,
      contentBlake3};
  return {std::move(name), std::move(treeEntry)};
}

TreePtr fromRawTree(
    const sapling::Tree* tree,
    const ObjectId& edenTreeId,
    RelativePathPiece path,
    HgObjectIdFormat hgObjectIdFormat) {
  Tree::container entries{kPathMapDefaultCaseSensitive};

  entries.reserve(tree->entries.size());
  for (uintptr_t i = 0; i < tree->entries.size(); i++) {
    try {
      auto entry = fromRawTreeEntry(tree->entries[i], path, hgObjectIdFormat);
      entries.emplace(entry.first, std::move(entry.second));
    } catch (const PathComponentContainsDirectorySeparator& ex) {
      XLOGF(WARN, "Ignoring directory entry: {}", ex.what());
    }
  }
  if (tree->aux_data.digest_size != 0) {
    XLOGF(
        DBG5,
        "Tree aux data returned from Sapling backing store when tree(id={}) is queried",
        edenTreeId);
    return std::make_shared<TreePtr::element_type>(
        edenTreeId,
        std::move(entries),
        std::make_shared<TreeAuxDataPtr::element_type>(
            Hash32{tree->aux_data.digest_hash}, tree->aux_data.digest_size));
  }
  XLOGF(
      DBG5,
      "No tree aux data returned from Sapling backing store when tree(id={}) is queried",
      edenTreeId);
  return std::make_shared<TreePtr::element_type>(
      std::move(entries), edenTreeId);
}

} // namespace

HgImportTraceEvent::HgImportTraceEvent(
    uint64_t unique,
    EventType eventType,
    ResourceType resourceType,
    const HgProxyHash& proxyHash,
    ImportPriority::Class priority,
    ObjectFetchContext::Cause cause,
    OptionalProcessId pid,
    std::optional<ObjectFetchContext::FetchedSource> fetchedSource)
    : unique{unique},
      manifestNodeId{proxyHash.revHash()},
      eventType{eventType},
      resourceType{resourceType},
      importPriority{priority},
      importCause{cause},
      pid{pid},
      fetchedSource{fetchedSource} {
  auto hgPath = proxyHash.path().view();
  // TODO: If HgProxyHash (and correspondingly ObjectId) used an immutable,
  // refcounted string, we wouldn't need to allocate here.
  path.reset(new char[hgPath.size() + 1]);
  memcpy(path.get(), hgPath.data(), hgPath.size());
  path[hgPath.size()] = 0;
}

SaplingBackingStore::SaplingBackingStore(
    AbsolutePathPiece repository,
    AbsolutePathPiece mount,
    std::shared_ptr<LocalStore> localStore,
    EdenStatsPtr stats,
    UnboundedQueueExecutor* serverThreadPool,
    std::shared_ptr<ReloadableConfig> config,
    std::unique_ptr<SaplingBackingStoreOptions> runtimeOptions,
    std::shared_ptr<StructuredLogger> structuredLogger,
    std::unique_ptr<BackingStoreLogger> logger,
    FaultInjector* FOLLY_NONNULL faultInjector)
    : localStore_(std::move(localStore)),
      stats_(stats.copy()),
      config_(config),
      serverThreadPool_(serverThreadPool),
      queue_(std::move(config)),
      structuredLogger_{std::move(structuredLogger)},
      logger_(std::move(logger)),
      faultInjector_{*faultInjector},
      localStoreCachingPolicy_{constructLocalStoreCachingPolicy()},
      runtimeOptions_(computeRuntimeOptions(std::move(runtimeOptions))),
      activityBuffer_{
          config_->getEdenConfig()->hgActivityBufferSize.getValue()},
      traceBus_{TraceBus<HgImportTraceEvent>::create(
          "hg",
          config_->getEdenConfig()->HgTraceBusCapacity.getValue())},
      store_{repository.view(), mount.view()} {
  uint8_t numberThreads =
      config_->getEdenConfig()->numBackingstoreThreads.getValue();
  if (!numberThreads) {
    XLOG(
        WARN,
        "SaplingBackingStore configured to use 0 threads. Invalid, using one thread instead");
    numberThreads = 1;
  }
  threads_.reserve(numberThreads);
  for (uint16_t i = 0; i < numberThreads; i++) {
    threads_.emplace_back(&SaplingBackingStore::processRequest, this);
  }

  hgTraceHandle_ = traceBus_->subscribeFunction(
      folly::to<std::string>("hg-activitybuffer-", getRepoName().value_or("")),
      [this](const HgImportTraceEvent& event) { this->processHgEvent(event); });

  if (config_->getEdenConfig()->enableOBCOnEden.getValue()) {
    initializeOBCCounters();
  }
}

/**
 * Create an SaplingBackingStore suitable for use in unit tests. It uses an
 * inline executor to process loaded objects rather than the thread pools used
 * in production Eden.
 */
SaplingBackingStore::SaplingBackingStore(
    AbsolutePathPiece repository,
    AbsolutePathPiece mount,
    std::shared_ptr<LocalStore> localStore,
    EdenStatsPtr stats,
    folly::InlineExecutor* inlineExecutor,
    std::shared_ptr<ReloadableConfig> config,
    std::unique_ptr<SaplingBackingStoreOptions> runtimeOptions,
    std::shared_ptr<StructuredLogger> structuredLogger,
    std::unique_ptr<BackingStoreLogger> logger,
    FaultInjector* FOLLY_NONNULL faultInjector)
    : localStore_(std::move(localStore)),
      stats_(std::move(stats)),
      config_(config),
      serverThreadPool_(inlineExecutor),
      queue_(std::move(config)),
      structuredLogger_{std::move(structuredLogger)},
      logger_(std::move(logger)),
      faultInjector_{*faultInjector},
      localStoreCachingPolicy_{constructLocalStoreCachingPolicy()},
      runtimeOptions_(std::move(runtimeOptions)),
      activityBuffer_{
          config_->getEdenConfig()->hgActivityBufferSize.getValue()},
      traceBus_{TraceBus<HgImportTraceEvent>::create(
          "hg",
          config_->getEdenConfig()->HgTraceBusCapacity.getValue())},
      store_{repository.view(), mount.view()} {
  uint8_t numberThreads =
      config_->getEdenConfig()->numBackingstoreThreads.getValue();
  if (!numberThreads) {
    XLOG(
        WARN,
        "SaplingBackingStore configured to use 0 threads. Invalid, using one thread instead");
    numberThreads = 1;
  }
  threads_.reserve(numberThreads);
  for (uint16_t i = 0; i < numberThreads; i++) {
    threads_.emplace_back(&SaplingBackingStore::processRequest, this);
  }

  hgTraceHandle_ = traceBus_->subscribeFunction(
      folly::to<std::string>("hg-activitybuffer-", getRepoName().value_or("")),
      [this](const HgImportTraceEvent& event) { this->processHgEvent(event); });

  if (config_->getEdenConfig()->enableOBCOnEden.getValue()) {
    initializeOBCCounters();
  }
}

SaplingBackingStore::~SaplingBackingStore() {
  queue_.stop();
  for (auto& thread : threads_) {
    thread.join();
  }
}

void SaplingBackingStore::initializeOBCCounters() {
  std::string repoName = store_.getRepoName().data();
  // Get the hostname without the ".facebook.com" suffix
  auto hostname = facebook::network::getLocalHost(/*stripFbDomain=*/true);
  getBlobPerRepoLatencies_ = monitoring::OBCP99P95P50(
      monitoring::OdsCategoryId::ODS_EDEN,
      fmt::format("eden.store.sapling.fetch_blob_{}_us", repoName),
      {hostname});
  getTreePerRepoLatencies_ = monitoring::OBCP99P95P50(
      monitoring::OdsCategoryId::ODS_EDEN,
      fmt::format("eden.store.sapling.fetch_tree_{}_us", repoName),
      {hostname});
  isOBCEnabled_ = true;
}

BackingStore::LocalStoreCachingPolicy
SaplingBackingStore::constructLocalStoreCachingPolicy() {
  using PolicyType =
      std::underlying_type_t<BackingStore::LocalStoreCachingPolicy>;
  PolicyType result =
      static_cast<PolicyType>(BackingStore::LocalStoreCachingPolicy::NoCaching);

  if (config_->getEdenConfig()->hgForceDisableLocalStoreCaching.getValue()) {
    // TODO: Instead of returning a NoCaching policy, we should just avoid
    // creating the LocalStore object if this config is set
    return static_cast<BackingStore::LocalStoreCachingPolicy>(result);
  }

  bool shouldCacheTrees =
      config_->getEdenConfig()->hgEnableTreeLocalStoreCaching.getValue();
  bool shouldCacheBlobs =
      config_->getEdenConfig()->hgEnableBlobLocalStoreCaching.getValue();
  bool shouldCacheBlobAuxData =
      config_->getEdenConfig()->hgEnableBlobMetaLocalStoreCaching.getValue();
  bool shouldCacheTreeAuxData =
      config_->getEdenConfig()->hgEnableTreeMetaLocalStoreCaching.getValue();

  if (shouldCacheTrees) {
    result |=
        static_cast<PolicyType>(BackingStore::LocalStoreCachingPolicy::Trees);
  }

  if (shouldCacheBlobs) {
    result |=
        static_cast<PolicyType>(BackingStore::LocalStoreCachingPolicy::Blobs);
  }

  if (shouldCacheBlobAuxData) {
    result |= static_cast<PolicyType>(
        BackingStore::LocalStoreCachingPolicy::BlobAuxData);
  }

  if (shouldCacheTreeAuxData) {
    result |= static_cast<PolicyType>(
        BackingStore::LocalStoreCachingPolicy::TreeAuxData);
  }
  return static_cast<BackingStore::LocalStoreCachingPolicy>(result);
}

void SaplingBackingStore::processHgEvent(const HgImportTraceEvent& event) {
  switch (event.eventType) {
    case HgImportTraceEvent::QUEUE:
      // Create a new queued event
    case HgImportTraceEvent::START:
      // Override the queued event with start event
      outstandingHgEvents_.wlock()->insert_or_assign(event.unique, event);
      break;
    case HgImportTraceEvent::FINISH:
      outstandingHgEvents_.wlock()->erase(event.unique);
      break;
    default:
      EDEN_BUG() << "Unknown Hg trace event type: "
                 << enumValue(event.eventType);
  }
  activityBuffer_.addEvent(event);
}

void SaplingBackingStore::setPrefetchBlobCounters(
    ObjectFetchContextPtr context,
    ObjectFetchContext::FetchedSource fetchedSource,
    ObjectFetchContext::FetchResult fetchResult,
    folly::stop_watch<std::chrono::milliseconds> watch) {
  if (fetchResult == ObjectFetchContext::FetchResult::Failure) {
    stats_->increment(&SaplingBackingStoreStats::prefetchBlobFailure);
    return;
  }
  stats_->addDuration(&SaplingBackingStoreStats::prefetchBlob, watch.elapsed());

  if (fetchResult == ObjectFetchContext::FetchResult::Success) {
    stats_->increment(&SaplingBackingStoreStats::prefetchBlobSuccess);
  } else {
    EDEN_BUG() << "Unknown fetch request result: " << enumValue(fetchResult);
  }

  context->setFetchedSource(
      fetchedSource,
      ObjectFetchContext::ObjectType::PrefetchBlob,
      stats_.copy());
}

void SaplingBackingStore::setFetchBlobCounters(
    ObjectFetchContextPtr context,
    ObjectFetchContext::FetchedSource fetchedSource,
    ObjectFetchContext::FetchResult fetchResult,
    folly::stop_watch<std::chrono::milliseconds> watch) {
  if (fetchResult == ObjectFetchContext::FetchResult::Failure) {
    stats_->increment(&SaplingBackingStoreStats::fetchBlobFailure);
    if (store_.dogfoodingHost()) {
      stats_->increment(&SaplingBackingStoreStats::fetchBlobFailureDogfooding);
    }
    return;
  }

  if (isOBCEnabled_) {
    getBlobPerRepoLatencies_ += watch.elapsed().count();
  }
  stats_->addDuration(&SaplingBackingStoreStats::fetchBlob, watch.elapsed());

  if (fetchResult == ObjectFetchContext::FetchResult::Success) {
    stats_->increment(&SaplingBackingStoreStats::fetchBlobSuccess);
  } else {
    EDEN_BUG() << "Unknown fetch request result: " << enumValue(fetchResult);
  }

  context->setFetchedSource(
      fetchedSource, ObjectFetchContext::ObjectType::Blob, stats_.copy());

  if (store_.dogfoodingHost()) {
    stats_->increment(&SaplingBackingStoreStats::fetchBlobSuccessDogfooding);
  }
}

void SaplingBackingStore::setBlobCounters(
    ObjectFetchContextPtr context,
    SaplingImportRequest::FetchType fetchType,
    ObjectFetchContext::FetchedSource fetchedSource,
    ObjectFetchContext::FetchResult fetchResult,
    folly::stop_watch<std::chrono::milliseconds> watch) {
  switch (fetchType) {
    case SaplingImportRequest::FetchType::Prefetch:
      setPrefetchBlobCounters(
          context.copy(), fetchedSource, fetchResult, watch);
      break;

    case SaplingImportRequest::FetchType::Fetch:
      setFetchBlobCounters(context.copy(), fetchedSource, fetchResult, watch);
      break;
  }
}

void SaplingBackingStore::processBlobImportRequests(
    std::vector<std::shared_ptr<SaplingImportRequest>>&& requests) {
  XLOGF(DBG4, "Processing blob import batch size={}", requests.size());

  for (auto& request : requests) {
    auto* blobImport = request->getRequest<SaplingImportRequest::BlobImport>();

    // TODO: We could reduce the number of lock acquisitions by adding a batch
    // publish method.
    traceBus_->publish(HgImportTraceEvent::start(
        request->getUnique(),
        HgImportTraceEvent::BLOB,
        blobImport->proxyHash,
        request->getPriority().getClass(),
        request->getCause(),
        request->getPid()));

    XLOGF(DBG4, "Processing blob request for {}", blobImport->id);
  }

  getBlobBatch(requests, sapling::FetchMode::AllowRemote);
}

void SaplingBackingStore::getBlobBatch(
    const ImportRequestsList& importRequests,
    sapling::FetchMode fetchMode) {
  auto preparedRequests = prepareRequests<SaplingImportRequest::BlobImport>(
      importRequests, SaplingImportObject::BLOB);
  auto importRequestsMap = std::move(preparedRequests.first);
  auto requests = std::move(preparedRequests.second);
  folly::stop_watch<std::chrono::milliseconds> batchWatch;

  auto allowIgnoreResult =
      config_->getEdenConfig()->ignorePrefetchResult.getValue();

  store_.getBlobBatch(
      folly::range(requests),
      fetchMode,
      allowIgnoreResult,
      // store_->getBlobBatch is blocking, hence we can take these by reference.
      [&](size_t index, folly::Try<std::unique_ptr<folly::IOBuf>> content) {
        if (content.hasException()) {
          XLOGF(
              DBG4,
              "Failed to import node={} from EdenAPI (batch {}/{}): {}",
              folly::hexlify(requests[index].node),
              index,
              requests.size(),
              content.exception().what().toStdString());

          if (structuredLogger_) {
            structuredLogger_->logEvent(FetchMiss{
                store_.getRepoName(),
                FetchMiss::Blob,
                content.exception().what().toStdString(),
                false, // isRetry
                store_.dogfoodingHost()});
          }
        } else {
          XLOGF(
              DBG4,
              "Imported node={} from EdenAPI (batch: {}/{})",
              folly::hexlify(requests[index].node),
              index,
              requests.size());
        }

        const auto& nodeId = requests[index].node;
        XLOGF(DBG9, "Imported Blob node={}", folly::hexlify(nodeId));
        auto& [importRequestList, watch] = importRequestsMap[nodeId];
        auto result = content.hasException()
            ? folly::Try<BlobPtr>{content.exception()}
            : content.value()
            ? folly::Try{std::make_shared<BlobPtr::element_type>(
                  *content.value())}
            // Propagate null content as nullptr. This happens when we use the
            // IGNORE_RESULT flag during blob prefetching. I think nullptr is
            // "safer" than setting an empty blob since we want to be confident
            // that no code uses the blob thinking there is content.
            : folly::Try<BlobPtr>{nullptr};
        for (auto& importRequest : importRequestList) {
          importRequest->getPromise<BlobPtr>()->setWith(
              [&]() -> folly::Try<BlobPtr> { return result; });

          setBlobCounters(
              importRequest->getContext().copy(),
              importRequest->getFetchType(),
              ObjectFetchContext::FetchedSource::Unknown,
              content.hasException() ? ObjectFetchContext::FetchResult::Failure
                                     : ObjectFetchContext::FetchResult::Success,
              batchWatch);
        }

        // Make sure that we're stopping this watch.
        watch.reset();
      });
}

folly::Try<BlobPtr> SaplingBackingStore::getBlobFromBackingStore(
    const HgProxyHash& hgInfo,
    const ObjectFetchContextPtr& context,
    sapling::FetchMode fetchMode) {
  auto blob =
      store_.getBlob(hgInfo.byteHash(), hgInfo.path(), context, fetchMode);

  using GetBlobResult = folly::Try<BlobPtr>;

  if (blob.hasValue()) {
    if (blob.value()) {
      return GetBlobResult{
          std::make_shared<BlobPtr::element_type>(std::move(*blob.value()))};
    } else {
      return GetBlobResult{nullptr};
    }
  } else {
    return GetBlobResult{blob.exception()};
  }
}

void SaplingBackingStore::processTreeImportRequests(
    std::vector<std::shared_ptr<SaplingImportRequest>>&& requests) {
  for (auto& request : requests) {
    auto* treeImport = request->getRequest<SaplingImportRequest::TreeImport>();

    // TODO: We could reduce the number of lock acquisitions by adding a batch
    // publish method.
    traceBus_->publish(HgImportTraceEvent::start(
        request->getUnique(),
        HgImportTraceEvent::TREE,
        treeImport->proxyHash,
        request->getPriority().getClass(),
        request->getCause(),
        request->getPid()));

    XLOGF(DBG4, "Processing tree request for {}", treeImport->id);
  }

  getTreeBatch(requests, sapling::FetchMode::AllowRemote);
}

void SaplingBackingStore::getTreeBatch(
    const ImportRequestsList& importRequests,
    sapling::FetchMode fetch_mode) {
  folly::stop_watch<std::chrono::milliseconds> batchWatch;

  auto preparedRequests = prepareRequests<SaplingImportRequest::TreeImport>(
      importRequests, SaplingImportObject::TREE);
  auto importRequestsMap = std::move(preparedRequests.first);
  auto requests = std::move(preparedRequests.second);
  auto hgObjectIdFormat = config_->getEdenConfig()->hgObjectIdFormat.getValue();

  faultInjector_.check("SaplingBackingStore::getTreeBatch", "");
  store_.getTreeBatch(
      folly::range(requests),
      fetch_mode,
      // getTreeBatch is blocking, hence we can take these by
      // reference.
      [&](size_t index,
          folly::Try<std::shared_ptr<sapling::Tree>> content) mutable {
        if (content.hasException()) {
          XLOGF(
              DBG4,
              "Failed to import node={} from EdenAPI (batch tree {}/{}): {}",
              folly::hexlify(requests[index].node),
              index,
              requests.size(),
              content.exception().what().toStdString());
          stats_->increment(&SaplingBackingStoreStats::fetchTreeFailure);
          if (store_.dogfoodingHost()) {
            stats_->increment(
                &SaplingBackingStoreStats::fetchTreeFailureDogfooding);
          }
        } else {
          XLOGF(
              DBG4,
              "Imported node={} from EdenAPI (batch tree: {}/{})",
              folly::hexlify(requests[index].node),
              index,
              requests.size());
          stats_->increment(&SaplingBackingStoreStats::fetchTreeSuccess);
          if (store_.dogfoodingHost()) {
            stats_->increment(
                &SaplingBackingStoreStats::fetchTreeSuccessDogfooding);
          }
        }

        if (isOBCEnabled_) {
          getTreePerRepoLatencies_ += batchWatch.elapsed().count();
        }
        stats_->addDuration(
            &SaplingBackingStoreStats::fetchTree, batchWatch.elapsed());

        const auto& nodeId = requests[index].node;
        XLOGF(DBG9, "Imported Tree node={}", folly::hexlify(nodeId));
        auto& [importRequestList, watch] = importRequestsMap[nodeId];
        for (auto& importRequest : importRequestList) {
          auto* treeRequest =
              importRequest->getRequest<SaplingImportRequest::TreeImport>();
          importRequest->getPromise<TreePtr>()->setWith(
              [&]() -> folly::Try<TreePtr> {
                if (content.hasException()) {
                  return folly::Try<TreePtr>{content.exception()};
                }
                return folly::Try{fromRawTree(
                    content.value().get(),
                    treeRequest->id,
                    treeRequest->proxyHash.path(),
                    hgObjectIdFormat)};
              });
        }

        // Make sure that we're stopping this watch.
        watch.reset();
      });
}

template <typename T>
std::pair<
    SaplingBackingStore::ImportRequestsMap,
    std::vector<sapling::SaplingRequest>>
SaplingBackingStore::prepareRequests(
    const ImportRequestsList& importRequests,
    const SaplingImportObject& requestType) {
  // TODO: extract each ClientRequestInfo from importRequests into a
  // sapling::ClientRequestInfo and pass them with the corresponding
  // sapling::NodeId

  // Group requests by proxyHash to ensure no duplicates in fetch request to
  // SaplingNativeBackingStore.
  ImportRequestsMap importRequestsMap;
  for (const auto& importRequest : importRequests) {
    auto nodeId = importRequest->getRequest<T>()->proxyHash.byteHash();

    // Look for and log duplicates.
    const auto& importRequestsEntry = importRequestsMap.find(nodeId);
    if (importRequestsEntry != importRequestsMap.end()) {
      XLOGF(
          DBG9,
          "Duplicate {} fetch request with proxyHash: {}",
          stringOfSaplingImportObject(requestType),
          folly::StringPiece{nodeId});
      auto& importRequestList = importRequestsEntry->second.first;

      // Only look for mismatched requests if logging level is high enough.
      // Make sure this level is the same as the XLOG_IF statement below.
      if (XLOG_IS_ON(DBG9)) {
        // Log requests that do not have the same hash (ObjectId).
        // This happens when two paths (file or directory) have same content.
        for (const auto& priorRequest : importRequestList) {
          XLOGF_IF(
              DBG9,
              UNLIKELY(
                  priorRequest->template getRequest<T>()->id !=
                  importRequest->getRequest<T>()->id),
              "{} requests have the same proxyHash (HgProxyHash) but different id (ObjectId). "
              "This should not happen. Previous request: id='{}', proxyHash='{}', proxyHash.path='{}'; "
              "current request: id='{}', proxyHash ='{}', proxyHash.path='{}'.",
              stringOfSaplingImportObject(requestType),
              priorRequest->template getRequest<T>()->id.asHexString(),
              folly::hexlify(
                  priorRequest->template getRequest<T>()->proxyHash.byteHash()),
              priorRequest->template getRequest<T>()->proxyHash.path(),
              importRequest->getRequest<T>()->id.asHexString(),
              folly::hexlify(
                  importRequest->getRequest<T>()->proxyHash.byteHash()),
              importRequest->getRequest<T>()->proxyHash.path());
        }
      }

      importRequestList.emplace_back(importRequest);
    } else {
      std::vector<std::shared_ptr<SaplingImportRequest>> requests(
          {importRequest});
      switch (requestType) {
        case SaplingImportObject::TREE:
          importRequestsMap.emplace(
              nodeId, make_pair(requests, &liveBatchedTreeWatches_));
          break;
        case SaplingImportObject::TREE_AUX:
          importRequestsMap.emplace(
              nodeId, make_pair(requests, &liveBatchedTreeAuxWatches_));
          break;
        case SaplingImportObject::BLOB:
          importRequestsMap.emplace(
              nodeId, make_pair(requests, &liveBatchedBlobWatches_));
          break;
        case SaplingImportObject::BLOB_AUX:
          importRequestsMap.emplace(
              nodeId, make_pair(requests, &liveBatchedBlobAuxWatches_));
          break;
        // The following types cannot get here. It is just for completeness
        case SaplingImportObject::BATCHED_TREE:
        case SaplingImportObject::BATCHED_TREE_AUX:
        case SaplingImportObject::BATCHED_BLOB:
        case SaplingImportObject::BATCHED_BLOB_AUX:
        case SaplingImportObject::PREFETCH:
          break;
      }
    }
  }

  // Indexable vector of nodeIds - required by SaplingNativeBackingStore API.
  // In addition, we pass the fetchCause for each request. If we have multiple
  // fetchCauses for the same nodeID, we will take the highest priority one.
  //
  // NOTE: Currently, backingstore ignores the fetchCause of the request. In the
  // future, backingstore may use different fetchCauses to change the fetch
  // behavior.
  std::vector<sapling::SaplingRequest> requests;
  for (const auto& importRequestsIdPair : importRequestsMap) {
    const ImportRequestsList& importRequestsForId =
        importRequestsIdPair.second.first;
    ObjectFetchContext::Cause fetchCause =
        getHighestPriorityFetchCause(importRequestsForId);
    requests.push_back(sapling::SaplingRequest{
        importRequestsIdPair.first,
        importRequestsForId[0]->getRequest<T>()->proxyHash.path(),
        fetchCause,
        importRequestsForId[0]->getContext().copy(),
    });
  }

  return std::make_pair(std::move(importRequestsMap), std::move(requests));
}

// The priority is defined in ObjectFetchContext::Cause
// FS -> Thrift -> Prefetch -> Unknown
ObjectFetchContext::Cause SaplingBackingStore::getHighestPriorityFetchCause(
    const ImportRequestsList& importRequestsForId) const {
  ObjectFetchContext::Cause highestPriorityCause =
      ObjectFetchContext::Cause::Unknown;
  for (const auto& request : importRequestsForId) {
    if (request) {
      highestPriorityCause =
          std::max(highestPriorityCause, request->getCause());
    }
  }
  return highestPriorityCause;
}

void SaplingBackingStore::processBlobAuxImportRequests(
    std::vector<std::shared_ptr<SaplingImportRequest>>&& requests) {
  folly::stop_watch<std::chrono::milliseconds> watch;

  for (auto& request : requests) {
    auto* blobAuxImport =
        request->getRequest<SaplingImportRequest::BlobAuxImport>();

    // TODO: We could reduce the number of lock acquisitions by adding a batch
    // publish method.
    traceBus_->publish(HgImportTraceEvent::start(
        request->getUnique(),
        HgImportTraceEvent::BLOB_AUX,
        blobAuxImport->proxyHash,
        request->getPriority().getClass(),
        request->getCause(),
        request->getPid()));

    XLOGF(DBG4, "Processing blob aux request for {}", blobAuxImport->id);
  }

  getBlobAuxDataBatch(requests, sapling::FetchMode::AllowRemote);

  {
    for (auto& request : requests) {
      auto* promise = request->getPromise<BlobAuxDataPtr>();
      if (promise->isFulfilled()) {
        stats_->addDuration(
            &SaplingBackingStoreStats::fetchBlobAuxData, watch.elapsed());
        stats_->increment(&SaplingBackingStoreStats::fetchBlobAuxDataSuccess);
        continue;
      }

      // The code waiting on the promise will fallback to fetching the Blob to
      // compute the blob aux data. We can't trigger a blob fetch here without
      // the risk of running into a deadlock: if all import thread are in this
      // code path, there are no free importer to fetch blobs.
      stats_->increment(&SaplingBackingStoreStats::fetchBlobAuxDataFailure);
      promise->setValue(nullptr);
    }
  }
}

void SaplingBackingStore::processTreeAuxImportRequests(
    std::vector<std::shared_ptr<SaplingImportRequest>>&& requests) {
  folly::stop_watch<std::chrono::milliseconds> watch;

  for (auto& request : requests) {
    auto* treeAuxImport =
        request->getRequest<SaplingImportRequest::TreeAuxImport>();

    // TODO: We could reduce the number of lock acquisitions by adding a batch
    // publish method.
    traceBus_->publish(HgImportTraceEvent::start(
        request->getUnique(),
        HgImportTraceEvent::TREE_AUX,
        treeAuxImport->proxyHash,
        request->getPriority().getClass(),
        request->getCause(),
        request->getPid()));

    XLOGF(DBG4, "Processing tree aux request for {}", treeAuxImport->id);
  }

  getTreeAuxDataBatch(requests, sapling::FetchMode::AllowRemote);

  {
    for (auto& request : requests) {
      auto* promise = request->getPromise<TreeAuxDataPtr>();
      if (promise->isFulfilled()) {
        stats_->addDuration(
            &SaplingBackingStoreStats::fetchTreeAuxData, watch.elapsed());
        stats_->increment(&SaplingBackingStoreStats::fetchTreeAuxDataSuccess);
        continue;
      }

      stats_->increment(&SaplingBackingStoreStats::fetchTreeAuxDataFailure);
      promise->setValue(nullptr);
    }
  }
}

void SaplingBackingStore::getTreeAuxDataBatch(
    const ImportRequestsList& importRequests,
    sapling::FetchMode fetch_mode) {
  auto preparedRequests = prepareRequests<SaplingImportRequest::TreeAuxImport>(
      importRequests, SaplingImportObject::TREE_AUX);
  auto importRequestsMap = std::move(preparedRequests.first);
  auto requests = std::move(preparedRequests.second);

  store_.getTreeAuxDataBatch(
      folly::range(requests),
      fetch_mode,
      // store_.getTreeAuxDataBatch is blocking, hence we can take these by
      // reference.
      [&](size_t index,
          folly::Try<std::shared_ptr<sapling::TreeAuxData>> auxTry) {
        if (auxTry.hasException()) {
          XLOGF(
              DBG6,
              "Failed to import aux data node={} from EdenAPI (batch {}/{}): {}",
              folly::hexlify(requests[index].node),
              index,
              requests.size(),
              auxTry.exception().what().toStdString());
        } else {
          XLOGF(
              DBG6,
              "Imported aux data node={} from EdenAPI (batch: {}/{})",
              folly::hexlify(requests[index].node),
              index,
              requests.size());
        }

        if (auxTry.hasException()) {
          if (structuredLogger_) {
            structuredLogger_->logEvent(FetchMiss{
                store_.getRepoName(),
                FetchMiss::TreeAuxData,
                auxTry.exception().what().toStdString(),
                false, // isRetry
                store_.dogfoodingHost()});
          }

          return;
        }

        const auto& nodeId = requests[index].node;
        XLOGF(DBG9, "Imported TreeAuxData={}", folly::hexlify(nodeId));
        auto& [importRequestList, watch] = importRequestsMap[nodeId];
        folly::Try<TreeAuxDataPtr> result;
        if (auxTry.hasException()) {
          result = folly::Try<TreeAuxDataPtr>{auxTry.exception()};
        } else {
          auto& aux = auxTry.value();
          result = folly::Try{std::make_shared<TreeAuxDataPtr::element_type>(
              Hash32{std::move(aux->digest_hash)}, aux->digest_size)};
        }
        for (auto& importRequest : importRequestList) {
          importRequest->getPromise<TreeAuxDataPtr>()->setWith(
              [&]() -> folly::Try<TreeAuxDataPtr> { return result; });
        }

        // Make sure that we're stopping this watch.
        watch.reset();
      });
}

void SaplingBackingStore::getBlobAuxDataBatch(
    const ImportRequestsList& importRequests,
    sapling::FetchMode fetch_mode) {
  auto preparedRequests = prepareRequests<SaplingImportRequest::BlobAuxImport>(
      importRequests, SaplingImportObject::BLOB_AUX);
  auto importRequestsMap = std::move(preparedRequests.first);
  auto requests = std::move(preparedRequests.second);

  store_.getBlobAuxDataBatch(
      folly::range(requests),
      fetch_mode,
      // store_.getBlobAuxDataBatch is blocking, hence we can take these by
      // reference.
      [&](size_t index,
          folly::Try<std::shared_ptr<sapling::FileAuxData>> auxTry) {
        if (auxTry.hasException()) {
          XLOGF(
              DBG4,
              "Failed to import aux data node={} from EdenAPI (batch {}/{}): {}",
              folly::hexlify(requests[index].node),
              index,
              requests.size(),
              auxTry.exception().what().toStdString());
        } else {
          XLOGF(
              DBG4,
              "Imported aux data node={} from EdenAPI (batch: {}/{})",
              folly::hexlify(requests[index].node),
              index,
              requests.size());
        }

        if (auxTry.hasException()) {
          if (structuredLogger_ &&
              fetch_mode != sapling::FetchMode::RemoteOnly) {
            structuredLogger_->logEvent(FetchMiss{
                store_.getRepoName(),
                FetchMiss::BlobAuxData,
                auxTry.exception().what().toStdString(),
                false, // isRetry
                store_.dogfoodingHost()});
          }

          return;
        }

        const auto& nodeId = requests[index].node;
        XLOGF(DBG9, "Imported BlobAuxData={}", folly::hexlify(nodeId));
        auto& [importRequestList, watch] = importRequestsMap[nodeId];
        folly::Try<BlobAuxDataPtr> result;
        if (auxTry.hasException()) {
          result = folly::Try<BlobAuxDataPtr>{auxTry.exception()};
        } else {
          auto& aux = auxTry.value();
          result = folly::Try{std::make_shared<BlobAuxDataPtr::element_type>(
              Hash20{std::move(aux->content_sha1)},
              Hash32{std::move(aux->content_blake3)},
              aux->total_size)};
        }
        for (auto& importRequest : importRequestList) {
          importRequest->getPromise<BlobAuxDataPtr>()->setWith(
              [&]() -> folly::Try<BlobAuxDataPtr> { return result; });
        }

        // Make sure that we're stopping this watch.
        watch.reset();
      });
}

void SaplingBackingStore::processRequest() {
  folly::setThreadName("hgqueue");
  for (;;) {
    auto requests = queue_.dequeue();

    if (requests.empty()) {
      break;
    }

    const auto& first = requests.at(0);

    if (first->isType<SaplingImportRequest::BlobImport>()) {
      processBlobImportRequests(std::move(requests));
    } else if (first->isType<SaplingImportRequest::TreeImport>()) {
      processTreeImportRequests(std::move(requests));
    } else if (first->isType<SaplingImportRequest::BlobAuxImport>()) {
      processBlobAuxImportRequests(std::move(requests));
    } else if (first->isType<SaplingImportRequest::TreeAuxImport>()) {
      processTreeAuxImportRequests(std::move(requests));
    } else {
      XLOGF(DFATAL, "Unknown import request type: {}", first->getType());
    }
  }
}

ObjectComparison SaplingBackingStore::compareObjectsById(
    const ObjectId& one,
    const ObjectId& two) {
  // This is by far the common case, so check it first:
  if (one.bytesEqual(two)) {
    return ObjectComparison::Identical;
  }

  if (config_->getEdenConfig()->hgBijectiveBlobIds.getValue()) {
    // If one and two differ, and hg bijective blob IDs is enabled, then we know
    // the blob contents differ.
    return ObjectComparison::Different;
  }

  // Now parse the object IDs and read their rev hashes.
  auto oneProxy = HgProxyHash::load(
      localStore_.get(), one, "areObjectIdsEquivalent", *stats_);
  auto twoProxy = HgProxyHash::load(
      localStore_.get(), two, "areObjectIdsEquivalent", *stats_);

  // If the rev hashes are the same, we know the contents are the same.
  if (oneProxy.revHash() == twoProxy.revHash()) {
    return ObjectComparison::Identical;
  }

  // If rev hashes differ, and hg IDs aren't bijective, then we don't know
  // whether the IDs refer to the same contents or not.
  //
  // Mercurial's blob ids also include history aux data, so there may be
  // multiple different blob ids for the same file contents.
  return ObjectComparison::Unknown;
}

RootId SaplingBackingStore::parseRootId(folly::StringPiece rootId) {
  // rootId can be 20-byte binary or 40-byte hex. Canonicalize, unconditionally
  // returning 40-byte hex.
  return RootId{hash20FromThrift(rootId).toString()};
}

std::string SaplingBackingStore::renderRootId(const RootId& rootId) {
  // In memory, root IDs are stored as 40-byte hex. Thrift clients generally
  // expect 20-byte binary for Mercurial commit hashes, so re-encode that way.
  auto& value = rootId.value();
  if (value.size() == 40) {
    return folly::unhexlify(value);
  } else {
    XCHECK_EQ(0u, value.size());
    // Default-constructed RootId is the Mercurial null hash.
    return folly::unhexlify(kZeroHash.toString());
  }
}

ObjectId SaplingBackingStore::staticParseObjectId(folly::StringPiece objectId) {
  if (objectId.startsWith("proxy-")) {
    if (objectId.size() != 46) {
      throwf<std::invalid_argument>(
          "invalid proxy hash length: {}", objectId.size());
    }

    return ObjectId{folly::unhexlify<folly::fbstring>(objectId.subpiece(6))};
  }

  if (objectId.size() == 40) {
    return HgProxyHash::makeEmbeddedProxyHash2(Hash20{objectId});
  }

  if (objectId.size() < 41) {
    throwf<std::invalid_argument>("hg object ID too short: {}", objectId);
  }

  if (objectId[40] != ':') {
    throwf<std::invalid_argument>(
        "missing separator colon in hg object ID: {}", objectId);
  }

  Hash20 hgRevHash{objectId.subpiece(0, 40)};
  RelativePathPiece path{objectId.subpiece(41)};
  return HgProxyHash::makeEmbeddedProxyHash1(hgRevHash, path);
}

std::string SaplingBackingStore::staticRenderObjectId(
    const ObjectId& objectId) {
  if (auto proxyHash = HgProxyHash::tryParseEmbeddedProxyHash(objectId)) {
    if (proxyHash->path().empty()) {
      return folly::hexlify(proxyHash->byteHash());
    }
    return fmt::format(
        "{}:{}", folly::hexlify(proxyHash->byteHash()), proxyHash->path());
  }
  return fmt::format("proxy-{}", folly::hexlify(objectId.getBytes()));
}

folly::SemiFuture<BackingStore::GetTreeAuxResult>
SaplingBackingStore::getTreeAuxData(
    const ObjectId& id,
    const ObjectFetchContextPtr& context) {
  DurationScope<EdenStats> scope{
      stats_, &SaplingBackingStoreStats::getTreeAuxData};

  HgProxyHash proxyHash;
  try {
    proxyHash =
        HgProxyHash::load(localStore_.get(), id, "getTreeAuxData", *stats_);
  } catch (const std::exception&) {
    logMissingProxyHash();
    throw;
  }

  logBackingStoreFetch(
      *context,
      folly::Range{&proxyHash, 1},
      ObjectFetchContext::ObjectType::TreeAuxData);

  auto auxData = getLocalTreeAuxData(proxyHash);
  if (auxData.hasValue() && auxData.value()) {
    stats_->increment(&SaplingBackingStoreStats::fetchTreeAuxDataSuccess);
    stats_->increment(&SaplingBackingStoreStats::fetchTreeAuxDataLocal);
    return folly::makeSemiFuture(GetTreeAuxResult{
        std::move(auxData.value()), ObjectFetchContext::Origin::FromDiskCache});
  }

  return getTreeAuxDataEnqueue(id, proxyHash, context)
      .ensure([scope = std::move(scope)] {})
      .semi();
}

ImmediateFuture<BackingStore::GetTreeAuxResult>
SaplingBackingStore::getTreeAuxDataEnqueue(
    const ObjectId& id,
    const HgProxyHash& proxyHash,
    const ObjectFetchContextPtr& context) {
  auto getTreeAuxFuture = makeImmediateFutureWith([&] {
    XLOGF(
        DBG4,
        "making tree aux data import request for {}",
        proxyHash.path(),
        id);
    auto requestContext = context.copy();
    auto request = SaplingImportRequest::makeTreeAuxImportRequest(
        id, proxyHash, requestContext);
    auto unique = request->getUnique();

    auto importTracker =
        std::make_unique<RequestMetricsScope>(&pendingImportTreeAuxWatches_);
    traceBus_->publish(HgImportTraceEvent::queue(
        unique,
        HgImportTraceEvent::TREE_AUX,
        proxyHash,
        context->getPriority().getClass(),
        context->getCause(),
        context->getClientPid()));

    return queue_.enqueueTreeAux(std::move(request))
        .ensure([this,
                 unique,
                 proxyHash,
                 context = context.copy(),
                 importTracker = std::move(importTracker)]() {
          traceBus_->publish(HgImportTraceEvent::finish(
              unique,
              HgImportTraceEvent::TREE_AUX,
              proxyHash,
              context->getPriority().getClass(),
              context->getCause(),
              context->getClientPid(),
              context->getFetchedSource()));
        });
  });

  return std::move(getTreeAuxFuture)
      .thenTry([this, id](folly::Try<TreeAuxDataPtr>&& result) {
        this->queue_.markImportAsFinished<TreeAuxDataPtr::element_type>(
            id, result);
        auto treeAux = std::move(result).value();
        return GetTreeAuxResult{
            std::move(treeAux), ObjectFetchContext::Origin::FromNetworkFetch};
      });
}

folly::Try<TreeAuxDataPtr> SaplingBackingStore::getLocalTreeAuxData(
    const HgProxyHash& hgInfo) {
  auto auxData = store_.getTreeAuxData(hgInfo.byteHash(), true /*local_only*/);

  using GetTreeAuxDataResult = folly::Try<TreeAuxDataPtr>;

  if (auxData.hasValue()) {
    if (auxData.value()) {
      return GetTreeAuxDataResult{
          std::make_shared<TreeAuxDataPtr::element_type>(TreeAuxData{
              Hash32{std::move(auxData.value()->digest_hash)},
              auxData.value()->digest_size})};
    } else {
      return GetTreeAuxDataResult{nullptr};
    }
  } else {
    return GetTreeAuxDataResult{auxData.exception()};
  }
}

folly::SemiFuture<BackingStore::GetTreeResult> SaplingBackingStore::getTree(
    const ObjectId& id,
    const ObjectFetchContextPtr& context) {
  DurationScope<EdenStats> scope{stats_, &SaplingBackingStoreStats::getTree};

  HgProxyHash proxyHash;
  try {
    proxyHash = HgProxyHash::load(localStore_.get(), id, "getTree", *stats_);
  } catch (const std::exception&) {
    logMissingProxyHash();
    throw;
  }

  logBackingStoreFetch(
      *context,
      folly::Range{&proxyHash, 1},
      ObjectFetchContext::ObjectType::Tree);

  if (auto tree = getTreeLocal(id, context, proxyHash)) {
    XLOGF(
        DBG5,
        "imported tree of '{}', {} from hgcache",
        proxyHash.path(),
        proxyHash.revHash().toString());
    stats_->increment(&SaplingBackingStoreStats::fetchTreeSuccess);
    if (store_.dogfoodingHost()) {
      stats_->increment(&SaplingBackingStoreStats::fetchTreeSuccessDogfooding);
    }
    stats_->increment(&SaplingBackingStoreStats::fetchTreeLocal);
    return folly::makeSemiFuture(GetTreeResult{
        std::move(tree), ObjectFetchContext::Origin::FromDiskCache});
  }

  return getTreeEnqueue(id, proxyHash, context)
      .ensure([scope = std::move(scope)] {})
      .semi();
}

ImmediateFuture<BackingStore::GetTreeResult>
SaplingBackingStore::getTreeEnqueue(
    const ObjectId& id,
    const HgProxyHash& proxyHash,
    const ObjectFetchContextPtr& context) {
  auto getTreeFuture = makeImmediateFutureWith([&] {
    auto requestContext = context.copy();
    auto request = SaplingImportRequest::makeTreeImportRequest(
        id, proxyHash, requestContext);
    uint64_t unique = request->getUnique();

    auto importTracker =
        std::make_unique<RequestMetricsScope>(&pendingImportTreeWatches_);
    traceBus_->publish(HgImportTraceEvent::queue(
        unique,
        HgImportTraceEvent::TREE,
        proxyHash,
        context->getPriority().getClass(),
        context->getCause(),
        context->getClientPid()));

    return queue_.enqueueTree(std::move(request))
        .ensure([this,
                 unique,
                 proxyHash,
                 context = context.copy(),
                 importTracker = std::move(importTracker)]() {
          traceBus_->publish(HgImportTraceEvent::finish(
              unique,
              HgImportTraceEvent::TREE,
              proxyHash,
              context->getPriority().getClass(),
              context->getCause(),
              context->getClientPid(),
              context->getFetchedSource()));
        });
  });

  return std::move(getTreeFuture)
      .thenTry([this, id](folly::Try<TreePtr>&& result) {
        this->queue_.markImportAsFinished<TreePtr::element_type>(id, result);
        auto tree = std::move(result).value();
        return GetTreeResult{
            std::move(tree), ObjectFetchContext::Origin::FromNetworkFetch};
      });
}

TreePtr SaplingBackingStore::getTreeLocal(
    const ObjectId& edenTreeId,
    const ObjectFetchContextPtr& context,
    const HgProxyHash& proxyHash) {
  auto tree = store_.getTree(
      proxyHash.byteHash(),
      proxyHash.path(),
      context,
      sapling::FetchMode::LocalOnly);

  if (tree.hasValue() && tree.value()) {
    auto hgObjectIdFormat =
        config_->getEdenConfig()->hgObjectIdFormat.getValue();
    return fromRawTree(
        tree.value().get(), edenTreeId, proxyHash.path(), hgObjectIdFormat);
  }

  return nullptr;
}

folly::Try<TreePtr> SaplingBackingStore::getTreeRemote(
    const RelativePath& path,
    const Hash20& manifestId,
    const ObjectId& edenTreeId,
    const ObjectFetchContextPtr& context) {
  auto tree = store_.getTree(
      manifestId.getBytes(),
      path,
      context,
      sapling::FetchMode::RemoteOnly /*, sapling::ClientRequestInfo(context)*/);

  using GetTreeResult = folly::Try<TreePtr>;

  if (tree.hasValue()) {
    if (tree.value()) {
      auto hgObjectIdFormat =
          config_->getEdenConfig()->hgObjectIdFormat.getValue();
      return GetTreeResult{fromRawTree(
          tree.value().get(), edenTreeId, path, std::move(hgObjectIdFormat))};
    } else {
      return GetTreeResult{nullptr};
    }
  } else {
    return GetTreeResult{tree.exception()};
  }
}

folly::SemiFuture<BackingStore::GetBlobResult> SaplingBackingStore::getBlob(
    const ObjectId& id,
    const ObjectFetchContextPtr& context) {
  DurationScope<EdenStats> scope{stats_, &SaplingBackingStoreStats::getBlob};

  HgProxyHash proxyHash;
  try {
    proxyHash = HgProxyHash::load(localStore_.get(), id, "getBlob", *stats_);
  } catch (const std::exception&) {
    logMissingProxyHash();
    throw;
  }

  logBackingStoreFetch(
      *context,
      folly::Range{&proxyHash, 1},
      ObjectFetchContext::ObjectType::Blob);

  auto blob = getBlobLocal(proxyHash, context);
  if (blob.hasValue() && blob.value()) {
    stats_->increment(&SaplingBackingStoreStats::fetchBlobSuccess);
    if (store_.dogfoodingHost()) {
      stats_->increment(&SaplingBackingStoreStats::fetchBlobSuccessDogfooding);
    }
    stats_->increment(&SaplingBackingStoreStats::fetchBlobLocal);
    return folly::makeSemiFuture(GetBlobResult{
        std::move(blob.value()), ObjectFetchContext::Origin::FromDiskCache});
  }

  return getBlobEnqueue(
             id, proxyHash, context, SaplingImportRequest::FetchType::Fetch)
      .ensure([scope = std::move(scope)] {})
      .semi();
}

ImmediateFuture<BackingStore::GetBlobResult>
SaplingBackingStore::getBlobEnqueue(
    const ObjectId& id,
    const HgProxyHash& proxyHash,
    const ObjectFetchContextPtr& context,
    const SaplingImportRequest::FetchType fetch_type) {
  auto getBlobFuture = makeImmediateFutureWith([&] {
    XLOGF(
        DBG4,
        "making blob import request for {}, id is: {}",
        proxyHash.path(),
        id);
    auto requestContext = context.copy();
    auto request = SaplingImportRequest::makeBlobImportRequest(
        id, proxyHash, requestContext);
    request->setFetchType(fetch_type);
    auto unique = request->getUnique();
    std::unique_ptr<RequestMetricsScope> importTracker;
    switch (fetch_type) {
      case SaplingImportRequest::FetchType::Fetch:
        importTracker =
            std::make_unique<RequestMetricsScope>(&pendingImportBlobWatches_);
        break;
      case SaplingImportRequest::FetchType::Prefetch:
        importTracker = std::make_unique<RequestMetricsScope>(
            &pendingImportPrefetchWatches_);
        break;
    }
    traceBus_->publish(HgImportTraceEvent::queue(
        unique,
        HgImportTraceEvent::BLOB,
        proxyHash,
        context->getPriority().getClass(),
        context->getCause(),
        context->getClientPid()));

    return queue_.enqueueBlob(std::move(request))
        .ensure([this,
                 unique,
                 proxyHash,
                 context = context.copy(),
                 importTracker = std::move(importTracker)]() {
          traceBus_->publish(HgImportTraceEvent::finish(
              unique,
              HgImportTraceEvent::BLOB,
              proxyHash,
              context->getPriority().getClass(),
              context->getCause(),
              context->getClientPid(),
              context->getFetchedSource()));
        });
  });

  return std::move(getBlobFuture)
      .thenTry([this, id](folly::Try<BlobPtr>&& result) {
        this->queue_.markImportAsFinished<BlobPtr::element_type>(id, result);
        auto blob = std::move(result).value();
        return GetBlobResult{
            std::move(blob), ObjectFetchContext::Origin::FromNetworkFetch};
      });
}

folly::SemiFuture<BackingStore::GetBlobAuxResult>
SaplingBackingStore::getBlobAuxData(
    const ObjectId& id,
    const ObjectFetchContextPtr& context) {
  DurationScope<EdenStats> scope{
      stats_, &SaplingBackingStoreStats::getBlobAuxData};

  HgProxyHash proxyHash;
  try {
    proxyHash =
        HgProxyHash::load(localStore_.get(), id, "getBlobAuxData", *stats_);
  } catch (const std::exception&) {
    logMissingProxyHash();
    throw;
  }

  logBackingStoreFetch(
      *context,
      folly::Range{&proxyHash, 1},
      ObjectFetchContext::ObjectType::BlobAuxData);

  auto auxData = getLocalBlobAuxData(proxyHash);
  if (auxData.hasValue() && auxData.value()) {
    stats_->increment(&SaplingBackingStoreStats::fetchBlobAuxDataSuccess);
    stats_->increment(&SaplingBackingStoreStats::fetchBlobAuxDataLocal);
    return folly::makeSemiFuture(GetBlobAuxResult{
        std::move(auxData.value()), ObjectFetchContext::Origin::FromDiskCache});
  }

  return getBlobAuxDataEnqueue(id, proxyHash, context)
      .ensure([scope = std::move(scope)] {})
      .semi();
}

ImmediateFuture<BackingStore::GetBlobAuxResult>
SaplingBackingStore::getBlobAuxDataEnqueue(
    const ObjectId& id,
    const HgProxyHash& proxyHash,
    const ObjectFetchContextPtr& context) {
  if (!config_->getEdenConfig()->fetchHgAuxMetadata.getValue()) {
    return BackingStore::GetBlobAuxResult{
        nullptr, ObjectFetchContext::Origin::NotFetched};
  }

  auto getBlobAuxFuture = makeImmediateFutureWith([&] {
    XLOGF(
        DBG4,
        "making blob meta import request for {}, id is: {}",
        proxyHash.path(),
        id);
    auto requestContext = context.copy();
    auto request = SaplingImportRequest::makeBlobAuxImportRequest(
        id, proxyHash, requestContext);
    auto unique = request->getUnique();

    auto importTracker =
        std::make_unique<RequestMetricsScope>(&pendingImportBlobAuxWatches_);
    traceBus_->publish(HgImportTraceEvent::queue(
        unique,
        HgImportTraceEvent::BLOB_AUX,
        proxyHash,
        context->getPriority().getClass(),
        context->getCause(),
        context->getClientPid()));

    return queue_.enqueueBlobAux(std::move(request))
        .ensure([this,
                 unique,
                 proxyHash,
                 context = context.copy(),
                 importTracker = std::move(importTracker)]() {
          traceBus_->publish(HgImportTraceEvent::finish(
              unique,
              HgImportTraceEvent::BLOB_AUX,
              proxyHash,
              context->getPriority().getClass(),
              context->getCause(),
              context->getClientPid(),
              context->getFetchedSource()));
        });
  });

  return std::move(getBlobAuxFuture)
      .thenTry([this, id](folly::Try<BlobAuxDataPtr>&& result) {
        this->queue_.markImportAsFinished<BlobAuxDataPtr::element_type>(
            id, result);
        auto blobAux = std::move(result).value();
        return GetBlobAuxResult{
            std::move(blobAux), ObjectFetchContext::Origin::FromNetworkFetch};
      });
}

folly::Try<BlobAuxDataPtr> SaplingBackingStore::getLocalBlobAuxData(
    const HgProxyHash& hgInfo) {
  auto auxData = store_.getBlobAuxData(hgInfo.byteHash(), true /*local_only*/);

  using GetBlobAuxDataResult = folly::Try<BlobAuxDataPtr>;

  if (auxData.hasValue()) {
    if (auxData.value()) {
      return GetBlobAuxDataResult{
          std::make_shared<BlobAuxDataPtr::element_type>(BlobAuxData{
              Hash20{std::move(auxData.value()->content_sha1)},
              Hash32{std::move(auxData.value()->content_blake3)},
              auxData.value()->total_size})};
    } else {
      return GetBlobAuxDataResult{nullptr};
    }
  } else {
    return GetBlobAuxDataResult{auxData.exception()};
  }
}

ImmediateFuture<BackingStore::GetRootTreeResult>
SaplingBackingStore::getRootTree(
    const RootId& rootId,
    const ObjectFetchContextPtr& context) {
  folly::stop_watch<std::chrono::milliseconds> watch;
  ObjectId commitId = hashFromRootId(rootId);

  return localStore_
      ->getImmediateFuture(KeySpace::HgCommitToTreeFamily, commitId)
      .thenValue(
          [this, commitId, context = context.copy(), watch](StoreResult result)
              -> folly::SemiFuture<BackingStore::GetRootTreeResult> {
            if (!result.isValid()) {
              return importTreeManifest(
                         commitId,
                         context,
                         ObjectFetchContext::ObjectType::RootTree)
                  .thenValue([this, commitId, watch](TreePtr rootTree) {
                    XLOGF(
                        DBG1,
                        "imported mercurial commit {} as tree {}",
                        commitId,
                        rootTree->getObjectId());
                    stats_->addDuration(
                        &SaplingBackingStoreStats::getRootTree,
                        watch.elapsed());
                    localStore_->put(
                        KeySpace::HgCommitToTreeFamily,
                        commitId,
                        rootTree->getObjectId().getBytes());
                    return BackingStore::GetRootTreeResult{
                        rootTree, rootTree->getObjectId()};
                  });
            }

            auto rootTreeHash = HgProxyHash::load(
                localStore_.get(),
                ObjectId{result.bytes()},
                "getRootTree",
                *stats_);
            return importTreeManifestImpl(
                       rootTreeHash.revHash(),
                       context,
                       ObjectFetchContext::ObjectType::RootTree)
                .thenValue([this, watch](TreePtr tree) {
                  stats_->addDuration(
                      &SaplingBackingStoreStats::getRootTree, watch.elapsed());
                  return BackingStore::GetRootTreeResult{
                      tree, tree->getObjectId()};
                });
          });
}

folly::Future<TreePtr> SaplingBackingStore::importTreeManifest(
    const ObjectId& commitId,
    const ObjectFetchContextPtr& context,
    const ObjectFetchContext::ObjectType type) {
  return folly::via(
             serverThreadPool_,
             [this, commitId] { return getManifestNode(commitId); })
      .thenValue([this, commitId, fetchContext = context.copy(), type](
                     auto manifestNode) {
        if (!manifestNode.has_value()) {
          auto ew = folly::exception_wrapper{std::runtime_error{
              "Manifest node could not be found for commitId"}};
          return folly::makeFuture<TreePtr>(std::move(ew));
        }
        XLOGF(
            DBG2,
            "commit {} has manifest node {}",
            commitId,
            manifestNode.value());
        return importTreeManifestImpl(
            *std::move(manifestNode), fetchContext, type);
      });
}

std::optional<Hash20> SaplingBackingStore::getManifestNode(
    const ObjectId& commitId) {
  auto manifestNode = store_.getManifestNode(commitId.getBytes());
  if (!manifestNode.has_value()) {
    XLOGF(DBG2, "Error while getting manifest node from datapackstore");
    return std::nullopt;
  }
  return Hash20(*std::move(manifestNode));
}

folly::Future<TreePtr> SaplingBackingStore::importTreeManifestImpl(
    Hash20 manifestNode,
    const ObjectFetchContextPtr& context,
    const ObjectFetchContext::ObjectType type) {
  // Record that we are at the root for this node
  RelativePathPiece path{};
  auto hgObjectIdFormat = config_->getEdenConfig()->hgObjectIdFormat.getValue();

  ObjectId objectId;

  switch (hgObjectIdFormat) {
    case HgObjectIdFormat::WithPath:
      objectId = HgProxyHash::makeEmbeddedProxyHash1(manifestNode, path);
      break;

    case HgObjectIdFormat::HashOnly:
      objectId = HgProxyHash::makeEmbeddedProxyHash2(manifestNode);
      break;
  }

  // try SaplingNativeBackingStore
  auto tree = getTreeFromBackingStore(
      path.copy(), manifestNode, objectId, context.copy(), type);
  bool success = tree.hasValue();

  // record stats
  switch (type) {
    case ObjectFetchContext::ObjectType::Tree:
      // getTree never gets here. We add this case only for completeness
      stats_->increment(
          success ? &SaplingBackingStoreStats::fetchTreeSuccess
                  : &SaplingBackingStoreStats::fetchTreeFailure);
      break;
    case ObjectFetchContext::ObjectType::RootTree:
      stats_->increment(
          success ? &SaplingBackingStoreStats::getRootTreeSuccess
                  : &SaplingBackingStoreStats::getRootTreeFailure);
      break;
    case ObjectFetchContext::ObjectType::ManifestForRoot:
      stats_->increment(
          success ? &SaplingBackingStoreStats::importManifestForRootSuccess
                  : &SaplingBackingStoreStats::importManifestForRootFailure);
      break;
      // The following types cannot get here. It is just for completeness
    case ObjectFetchContext::TreeAuxData:
    case ObjectFetchContext::Blob:
    case ObjectFetchContext::BlobAuxData:
    case ObjectFetchContext::ObjectType::PrefetchBlob:
    case ObjectFetchContext::kObjectTypeEnumMax:
      break;
  }
  if (store_.dogfoodingHost()) {
    stats_->increment(
        success ? &SaplingBackingStoreStats::fetchTreeSuccessDogfooding
                : &SaplingBackingStoreStats::fetchTreeFailureDogfooding);
  }

  if (tree.hasValue()) {
    XLOGF(
        DBG4,
        "imported tree node={} path={} from SaplingNativeBackingStore",
        manifestNode,
        path);
    return folly::makeFuture(std::move(tree.value()));
  } else {
    return folly::makeFuture<TreePtr>(tree.exception());
  }
}

folly::Try<TreePtr> SaplingBackingStore::getTreeFromBackingStore(
    const RelativePath& path,
    const Hash20& manifestId,
    const ObjectId& edenTreeId,
    ObjectFetchContextPtr context,
    const ObjectFetchContext::ObjectType type) {
  folly::Try<std::shared_ptr<sapling::Tree>> tree;
  sapling::FetchMode fetch_mode = sapling::FetchMode::AllowRemote;
  // For root trees we will try getting the tree locally first.  This allows
  // us to catch when Mercurial might have just written a tree to the store,
  // and refresh the store so that the store can pick it up.  We don't do
  // this for all trees, as it would cause a lot of additional work on every
  // cache miss, and just doing it for root trees is sufficient to detect the
  // scenario where Mercurial just wrote a brand new tree.
  if (path.empty()) {
    fetch_mode = sapling::FetchMode::LocalOnly;
  }
  tree = store_.getTree(manifestId.getBytes(), path, context, fetch_mode);
  if (tree.hasValue() && !tree.value() &&
      fetch_mode == sapling::FetchMode::LocalOnly) {
    // Mercurial might have just written the tree to the store. Refresh the
    // store and try again, this time allowing remote fetches.
    store_.flush();
    fetch_mode = sapling::FetchMode::AllowRemote;
    tree = store_.getTree(manifestId.getBytes(), path, context, fetch_mode);
  }

  using GetTreeResult = folly::Try<TreePtr>;

  if (tree.hasValue()) {
    if (!tree.value()) {
      return GetTreeResult{std::runtime_error{
          fmt::format("no tree found for {} (path={})", manifestId, path)}};
    }

    auto hgObjectIdFormat =
        config_->getEdenConfig()->hgObjectIdFormat.getValue();
    switch (fetch_mode) {
      case sapling::FetchMode::LocalOnly:
        context->setFetchedSource(
            ObjectFetchContext::FetchedSource::Local, type, stats_.copy());
        break;
      case sapling::FetchMode::RemoteOnly:
        context->setFetchedSource(
            ObjectFetchContext::FetchedSource::Remote, type, stats_.copy());
        break;
      case sapling::FetchMode::AllowRemote:
        context->setFetchedSource(
            ObjectFetchContext::FetchedSource::Unknown, type, stats_.copy());
        break;
    }
    return GetTreeResult{fromRawTree(
        tree.value().get(), edenTreeId, path, std::move(hgObjectIdFormat))};
  } else {
    return GetTreeResult{tree.exception()};
  }
}

folly::SemiFuture<folly::Unit> SaplingBackingStore::prefetchBlobs(
    ObjectIdRange ids,
    const ObjectFetchContextPtr& context) {
  return HgProxyHash::getBatch(localStore_.get(), ids, *stats_)
      // The caller guarantees that ids will live at least longer than this
      // future, thus we don't need to deep-copy it.
      .thenTry([context = context.copy(), this, ids](
                   folly::Try<std::vector<HgProxyHash>> tryHashes) {
        if (tryHashes.hasException()) {
          logMissingProxyHash();
        }
        auto& proxyHashes = tryHashes.value();

        logBackingStoreFetch(
            *context,
            folly::Range{proxyHashes.data(), proxyHashes.size()},
            ObjectFetchContext::ObjectType::Blob);

        // Do not check for whether blobs are already present locally, this
        // check is useful for latency oriented workflows, not for throughput
        // oriented ones. Mercurial will anyway not re-fetch a blob that is
        // already present locally, so the check for local blob is pure
        // overhead when prefetching.
        std::vector<ImmediateFuture<GetBlobResult>> futures;
        futures.reserve(ids.size());

        for (size_t i = 0; i < ids.size(); i++) {
          const auto& id = ids[i];
          const auto& proxyHash = proxyHashes[i];

          futures.emplace_back(getBlobEnqueue(
              id,
              proxyHash,
              context,
              SaplingImportRequest::FetchType::Prefetch));
        }

        return collectAllSafe(std::move(futures)).unit();
      })
      .semi();
}

ImmediateFuture<BackingStore::GetGlobFilesResult>
SaplingBackingStore::getGlobFiles(
    const RootId& id,
    const std::vector<std::string>& globs,
    const std::vector<std::string>& prefixes) {
  folly::stop_watch<std::chrono::milliseconds> watch;
  using GetGlobFilesResult = folly::Try<GetGlobFilesResult>;
  auto globFilesResult = store_.getGlobFiles(id.value(), globs, prefixes);

  if (globFilesResult.hasValue()) {
    std::vector<std::string> files;
    auto globFiles = globFilesResult.value()->files;
    for (auto& file : globFiles) {
      files.emplace_back(file);
    }
    stats_->addDuration(
        &SaplingBackingStoreStats::fetchGlobFiles, watch.elapsed());
    stats_->increment(&SaplingBackingStoreStats::fetchGlobFilesSuccess);

    return GetGlobFilesResult{BackingStore::GetGlobFilesResult{files, id}};
  } else {
    stats_->increment(&SaplingBackingStoreStats::fetchGlobFilesFailure);
    return GetGlobFilesResult{globFilesResult.exception()};
  }
}

void SaplingBackingStore::logMissingProxyHash() {
  auto now = std::chrono::steady_clock::now();

  bool shouldLog = false;
  {
    auto last = lastMissingProxyHashLog_.wlock();
    if (now >= *last +
            config_->getEdenConfig()
                ->missingHgProxyHashLogInterval.getValue()) {
      shouldLog = true;
      *last = now;
    }
  }

  if (shouldLog) {
    structuredLogger_->logEvent(MissingProxyHash{});
  }
}

void SaplingBackingStore::logBackingStoreFetch(
    const ObjectFetchContext& context,
    folly::Range<HgProxyHash*> hashes,
    ObjectFetchContext::ObjectType type) {
  const auto& logFetchPathRegex =
      config_->getEdenConfig()->logObjectFetchPathRegex.getValue();

  if (logFetchPathRegex) {
    for (const auto& hash : hashes) {
      auto path = hash.path();
      auto pathPiece = path.view();

      if (RE2::PartialMatch(
              re2::StringPiece{pathPiece.data(), pathPiece.size()},
              **logFetchPathRegex)) {
        logger_->logImport(context, path, type);
      }
    }
  }

  if (type != ObjectFetchContext::ObjectType::Tree &&
      isRecordingFetch_.load(std::memory_order_relaxed) &&
      context.getCause() != ObjectFetchContext::Cause::Prefetch) {
    auto guard = fetchedFilePaths_.wlock();
    for (const auto& hash : hashes) {
      guard->emplace(hash.path().view());
    }
  }
}

size_t SaplingBackingStore::getImportMetric(
    RequestMetricsScope::RequestStage stage,
    SaplingImportObject object,
    RequestMetricsScope::RequestMetric metric) const {
  return RequestMetricsScope::getMetricFromWatches(
      metric, getImportWatches(stage, object));
}

RequestMetricsScope::LockedRequestWatchList&
SaplingBackingStore::getImportWatches(
    RequestMetricsScope::RequestStage stage,
    SaplingImportObject object) const {
  switch (stage) {
    case RequestMetricsScope::RequestStage::PENDING:
      return getPendingImportWatches(object);
    case RequestMetricsScope::RequestStage::LIVE:
      return getLiveImportWatches(object);
  }
  EDEN_BUG() << "unknown sapling import stage " << enumValue(stage);
}

RequestMetricsScope::LockedRequestWatchList&
SaplingBackingStore::getPendingImportWatches(SaplingImportObject object) const {
  switch (object) {
    case SaplingImportObject::BLOB:
    case SaplingImportObject::BATCHED_BLOB:
      return pendingImportBlobWatches_;
    case SaplingImportObject::TREE:
    case SaplingImportObject::BATCHED_TREE:
      return pendingImportTreeWatches_;
    case SaplingImportObject::BLOB_AUX:
    case SaplingImportObject::BATCHED_BLOB_AUX:
      return pendingImportBlobAuxWatches_;
    case SaplingImportObject::TREE_AUX:
    case SaplingImportObject::BATCHED_TREE_AUX:
      return pendingImportTreeAuxWatches_;
    case SaplingImportObject::PREFETCH:
      return pendingImportPrefetchWatches_;
  }
  EDEN_BUG() << "unknown sapling import object type "
             << static_cast<int>(object);
}

RequestMetricsScope::LockedRequestWatchList&
SaplingBackingStore::getLiveImportWatches(SaplingImportObject object) const {
  switch (object) {
    case SaplingImportObject::BLOB:
      return liveImportBlobWatches_;
    case SaplingImportObject::TREE:
      return liveImportTreeWatches_;
    case SaplingImportObject::BLOB_AUX:
      return liveImportBlobAuxWatches_;
    case SaplingImportObject::TREE_AUX:
      return liveImportTreeAuxWatches_;
    case SaplingImportObject::PREFETCH:
      return liveImportPrefetchWatches_;
    case SaplingImportObject::BATCHED_BLOB:
      return liveBatchedBlobWatches_;
    case SaplingImportObject::BATCHED_TREE:
      return liveBatchedTreeWatches_;
    case SaplingImportObject::BATCHED_BLOB_AUX:
      return liveBatchedBlobAuxWatches_;
    case SaplingImportObject::BATCHED_TREE_AUX:
      return liveBatchedTreeAuxWatches_;
  }
  EDEN_BUG() << "unknown sapling import object " << enumValue(object);
}

folly::StringPiece SaplingBackingStore::stringOfSaplingImportObject(
    SaplingImportObject object) {
  switch (object) {
    case SaplingImportObject::BLOB:
      return "blob";
    case SaplingImportObject::TREE:
      return "tree";
    case SaplingImportObject::BLOB_AUX:
      return "blobmeta";
    case SaplingImportObject::TREE_AUX:
      return "treemeta";
    case SaplingImportObject::BATCHED_BLOB:
      return "batched_blob";
    case SaplingImportObject::BATCHED_TREE:
      return "batched_tree";
    case SaplingImportObject::BATCHED_BLOB_AUX:
      return "batched_blobmeta";
    case SaplingImportObject::BATCHED_TREE_AUX:
      return "batched_treemeta";
    case SaplingImportObject::PREFETCH:
      return "prefetch";
  }
  EDEN_BUG() << "unknown sapling import object " << enumValue(object);
}

void SaplingBackingStore::startRecordingFetch() {
  isRecordingFetch_.store(true, std::memory_order_relaxed);
}

std::unordered_set<std::string> SaplingBackingStore::stopRecordingFetch() {
  isRecordingFetch_.store(false, std::memory_order_relaxed);
  std::unordered_set<std::string> paths;
  std::swap(paths, *fetchedFilePaths_.wlock());
  return paths;
}

ImmediateFuture<folly::Unit> SaplingBackingStore::importManifestForRoot(
    const RootId& rootId,
    const Hash20& manifestId,
    const ObjectFetchContextPtr& context) {
  // This method is used when the client informs us about a target manifest
  // that it is about to update to, for the scenario when a manifest has
  // just been created.  Since the manifest has just been created locally, and
  // aux data is only available remotely, there will be no aux data available
  // to prefetch.
  //
  // When the local store is populated with aux data for newly-created
  // manifests then we can update this so that is true when appropriate.
  /**
   * Import the root manifest for the specified revision using mercurial
   * treemanifest data.  This is called when the root manifest is provided
   * to EdenFS directly by the hg client.
   */
  folly::stop_watch<std::chrono::milliseconds> watch;
  auto commitId = hashFromRootId(rootId);
  return localStore_
      ->getImmediateFuture(KeySpace::HgCommitToTreeFamily, commitId)
      .thenValue(
          [this, commitId, manifestId, context = context.copy(), watch](
              StoreResult result) -> folly::Future<folly::Unit> {
            if (result.isValid()) {
              // We have already imported this commit, nothing to do.
              return folly::unit;
            }

            return importTreeManifestImpl(
                       manifestId,
                       context,
                       ObjectFetchContext::ObjectType::ManifestForRoot)
                .thenValue([this, commitId, manifestId, watch](
                               TreePtr rootTree) {
                  XLOGF(
                      DBG3,
                      "imported mercurial commit {} with manifest {} as tree {}",
                      commitId,
                      manifestId,
                      rootTree->getObjectId());
                  stats_->addDuration(
                      &SaplingBackingStoreStats::importManifestForRoot,
                      watch.elapsed());
                  localStore_->put(
                      KeySpace::HgCommitToTreeFamily,
                      commitId,
                      rootTree->getObjectId().getBytes());
                });
          });
}

void SaplingBackingStore::periodicManagementTask() {
  flush();
}

namespace {
void dropBlobImportRequest(std::shared_ptr<SaplingImportRequest>& request) {
  auto* promise = request->getPromise<BlobPtr>();
  if (promise != nullptr) {
    if (!promise->isFulfilled()) {
      promise->setException(std::runtime_error("Request forcibly dropped"));
    }
  }
}

void dropTreeImportRequest(std::shared_ptr<SaplingImportRequest>& request) {
  auto* promise = request->getPromise<TreePtr>();
  if (promise != nullptr) {
    if (!promise->isFulfilled()) {
      promise->setException(std::runtime_error("Request forcibly dropped"));
    }
  }
}
} // namespace

int64_t SaplingBackingStore::dropAllPendingRequestsFromQueue() {
  auto requestVec = queue_.combineAndClearRequestQueues();
  for (auto& request : requestVec) {
    if (request->isType<SaplingImportRequest::BlobImport>()) {
      XLOG(DBG7, "Dropping blob request");
      dropBlobImportRequest(request);
    } else if (request->isType<SaplingImportRequest::TreeImport>()) {
      XLOG(DBG7, "Dropping tree request");
      dropTreeImportRequest(request);
    }
  }
  return requestVec.size();
}

} // namespace facebook::eden
