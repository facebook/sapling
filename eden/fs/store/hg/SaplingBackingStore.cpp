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
#include <folly/portability/GFlags.h>
#include <folly/system/ThreadName.h>

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
#include "eden/fs/model/BlobMetadata.h"
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

DEFINE_bool(
    hg_fetch_missing_trees,
    true,
    "Set this parameter to \"no\" to disable fetching missing treemanifest "
    "trees from the remote mercurial server.  This is generally only useful "
    "for testing/debugging purposes");

DEFINE_int32(
    num_hg_import_threads,
    // Why 8? 1 is materially slower but 24 is no better than 4 in a simple
    // microbenchmark that touches all files.  8 is better than 4 in the case
    // that we need to fetch a bunch from the network.
    // See benchmarks in the doc linked from D5067763.
    // Note that this number would benefit from occasional revisiting.
    8,
    "the number of sapling import threads per repo");

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

/**
 * Thread factory that sets thread name and initializes a thread local
 * Sapling retry state.
 */
class SaplingRetryThreadFactory : public folly::InitThreadFactory {
 public:
  SaplingRetryThreadFactory(
      AbsolutePathPiece repository,
      EdenStatsPtr stats,
      std::shared_ptr<StructuredLogger> logger)
      : folly::InitThreadFactory(
            std::make_shared<folly::NamedThreadFactory>("SaplingRetry"),
            [repository = AbsolutePath{repository},
             stats = std::move(stats),
             logger] {},
            [] {}) {}
};

sapling::SaplingNativeBackingStoreOptions computeSaplingOptions() {
  sapling::SaplingNativeBackingStoreOptions options{};
  options.allow_retries = false;
  return options;
}

sapling::SaplingNativeBackingStoreOptions computeTestSaplingOptions() {
  sapling::SaplingNativeBackingStoreOptions options{};
  options.allow_retries = false;
  return options;
}

std::unique_ptr<SaplingBackingStoreOptions> computeRuntimeOptions(
    std::unique_ptr<SaplingBackingStoreOptions> options) {
  options->ignoreFilteredPathsConfig =
      options->ignoreFilteredPathsConfig.value_or(false);
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

bool doFilteredPathsApply(
    bool ignoreFilteredPathsConfig,
    const std::unordered_set<RelativePath>& filteredPaths,
    const RelativePath& path) {
  return ignoreFilteredPathsConfig || filteredPaths.empty() ||
      filteredPaths.count(path) == 0;
}

TreePtr fromRawTree(
    const sapling::Tree* tree,
    const ObjectId& edenTreeId,
    RelativePathPiece path,
    HgObjectIdFormat hgObjectIdFormat,
    const std::unordered_set<RelativePath>& filteredPaths,
    bool ignoreFilteredPathsConfig) {
  Tree::container entries{kPathMapDefaultCaseSensitive};

  entries.reserve(tree->entries.size());
  for (uintptr_t i = 0; i < tree->entries.size(); i++) {
    try {
      auto entry = fromRawTreeEntry(tree->entries[i], path, hgObjectIdFormat);
      // TODO(xavierd): In the case where this checks becomes too hot, we may
      // need to change to a Trie like datastructure for fast filtering.
      if (doFilteredPathsApply(
              ignoreFilteredPathsConfig, filteredPaths, path + entry.first)) {
        entries.emplace(entry.first, std::move(entry.second));
      }
    } catch (const PathComponentContainsDirectorySeparator& ex) {
      XLOG(WARN) << "Ignoring directory entry: " << ex.what();
    }
  }
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
    OptionalProcessId pid)
    : unique{unique},
      manifestNodeId{proxyHash.revHash()},
      eventType{eventType},
      resourceType{resourceType},
      importPriority{priority},
      importCause{cause},
      pid{pid} {
  auto hgPath = proxyHash.path().view();
  // TODO: If HgProxyHash (and correspondingly ObjectId) used an immutable,
  // refcounted string, we wouldn't need to allocate here.
  path.reset(new char[hgPath.size() + 1]);
  memcpy(path.get(), hgPath.data(), hgPath.size());
  path[hgPath.size()] = 0;
}

SaplingBackingStore::SaplingBackingStore(
    AbsolutePathPiece repository,
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
      retryThreadPool_(std::make_unique<folly::CPUThreadPoolExecutor>(
          FLAGS_num_hg_import_threads,
          /* Eden performance will degrade when, for example, a status operation
           * causes a large number of import requests to be scheduled before a
           * lightweight operation needs to check the RocksDB cache. In that
           * case, the RocksDB threads can end up all busy inserting work into
           * the retry queue, preventing future requests that would hit cache
           * from succeeding.
           *
           * Thus, make the retry queue unbounded.
           *
           * In the long term, we'll want a more comprehensive approach to
           * bounding the parallelism of scheduled work.
           */
          std::make_unique<folly::UnboundedBlockingQueue<
              folly::CPUThreadPoolExecutor::CPUTask>>(),
          std::make_shared<SaplingRetryThreadFactory>(
              repository,
              stats.copy(),
              structuredLogger))),
      config_(config),
      serverThreadPool_(serverThreadPool),
      queue_(std::move(config)),
      structuredLogger_{std::move(structuredLogger)},
      logger_(std::move(logger)),
      faultInjector_{*faultInjector},
      localStoreCachingPolicy_{
          config_->getEdenConfig()->hgEnableBlobMetaLocalStoreCaching.getValue()
              ? LocalStoreCachingPolicy::TreesAndBlobMetadata
              : LocalStoreCachingPolicy::Trees},
      runtimeOptions_(computeRuntimeOptions(std::move(runtimeOptions))),
      activityBuffer_{
          config_->getEdenConfig()->hgActivityBufferSize.getValue()},
      traceBus_{TraceBus<HgImportTraceEvent>::create(
          "hg",
          config_->getEdenConfig()->HgTraceBusCapacity.getValue())},
      store_{repository.view(), computeSaplingOptions()} {
  uint8_t numberThreads =
      config_->getEdenConfig()->numBackingstoreThreads.getValue();
  if (!numberThreads) {
    XLOG(WARN)
        << "SaplingBackingStore configured to use 0 threads. Invalid, using one thread instead";
    numberThreads = 1;
  }
  threads_.reserve(numberThreads);
  for (uint16_t i = 0; i < numberThreads; i++) {
    threads_.emplace_back(&SaplingBackingStore::processRequest, this);
  }

  hgTraceHandle_ = traceBus_->subscribeFunction(
      folly::to<std::string>("hg-activitybuffer-", getRepoName().value_or("")),
      [this](const HgImportTraceEvent& event) {
        activityBuffer_.addEvent(event);
      });
}

/**
 * Create an SaplingBackingStore suitable for use in unit tests. It uses an
 * inline executor to process loaded objects rather than the thread pools used
 * in production Eden.
 */
SaplingBackingStore::SaplingBackingStore(
    AbsolutePathPiece repository,
    std::shared_ptr<LocalStore> localStore,
    EdenStatsPtr stats,
    std::shared_ptr<ReloadableConfig> config,
    std::unique_ptr<SaplingBackingStoreOptions> runtimeOptions,
    std::shared_ptr<StructuredLogger> structuredLogger,
    std::unique_ptr<BackingStoreLogger> logger,
    FaultInjector* FOLLY_NONNULL faultInjector)
    : localStore_(std::move(localStore)),
      stats_(std::move(stats)),
      retryThreadPool_{std::make_unique<folly::InlineExecutor>()},
      config_(config),
      serverThreadPool_(retryThreadPool_.get()),
      queue_(std::move(config)),
      structuredLogger_{std::move(structuredLogger)},
      logger_(std::move(logger)),
      faultInjector_{*faultInjector},
      localStoreCachingPolicy_{
          config_->getEdenConfig()->hgEnableBlobMetaLocalStoreCaching.getValue()
              ? LocalStoreCachingPolicy::TreesAndBlobMetadata
              : LocalStoreCachingPolicy::Trees},
      runtimeOptions_(std::move(runtimeOptions)),
      activityBuffer_{
          config_->getEdenConfig()->hgActivityBufferSize.getValue()},
      traceBus_{TraceBus<HgImportTraceEvent>::create(
          "hg",
          config_->getEdenConfig()->HgTraceBusCapacity.getValue())},
      store_{repository.view(), computeTestSaplingOptions()} {
  uint8_t numberThreads =
      config_->getEdenConfig()->numBackingstoreThreads.getValue();
  if (!numberThreads) {
    XLOG(WARN)
        << "SaplingBackingStore configured to use 0 threads. Invalid, using one thread instead";
    numberThreads = 1;
  }
  threads_.reserve(numberThreads);
  for (uint16_t i = 0; i < numberThreads; i++) {
    threads_.emplace_back(&SaplingBackingStore::processRequest, this);
  }

  hgTraceHandle_ = traceBus_->subscribeFunction(
      folly::to<std::string>("hg-activitybuffer-", getRepoName().value_or("")),
      [this](const HgImportTraceEvent& event) {
        activityBuffer_.addEvent(event);
      });
}

SaplingBackingStore::~SaplingBackingStore() {
  queue_.stop();
  for (auto& thread : threads_) {
    thread.join();
  }
}

void SaplingBackingStore::processBlobImportRequests(
    std::vector<std::shared_ptr<SaplingImportRequest>>&& requests) {
  folly::stop_watch<std::chrono::milliseconds> watch;

  XLOG(DBG4) << "Processing blob import batch size=" << requests.size();

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

    XLOGF(DBG4, "Processing blob request for {}", blobImport->hash);
  }

  std::vector<std::shared_ptr<SaplingImportRequest>> retryRequest;
  retryRequest.reserve(requests.size());
  if (config_->getEdenConfig()->allowRemoteGetBatch.getValue()) {
    getBlobBatch(requests, sapling::FetchMode::AllowRemote);
    retryRequest = std::move(requests);
  } else {
    getBlobBatch(requests, sapling::FetchMode::LocalOnly);

    for (auto& request : requests) {
      auto* promise = request->getPromise<BlobPtr>();
      if (promise->isFulfilled()) {
        XLOG(DBG4)
            << "Blob found in Sapling local for "
            << request->getRequest<SaplingImportRequest::BlobImport>()->hash;
        stats_->addDuration(
            &SaplingBackingStoreStats::fetchBlob, watch.elapsed());
      } else {
        retryRequest.emplace_back(std::move(request));
      }
    }

    getBlobBatch(retryRequest, sapling::FetchMode::RemoteOnly);
  }

  {
    std::vector<folly::SemiFuture<folly::Unit>> futures;
    futures.reserve(retryRequest.size());

    for (auto& request : retryRequest) {
      auto* promise = request->getPromise<BlobPtr>();
      if (promise->isFulfilled()) {
        if (!config_->getEdenConfig()->allowRemoteGetBatch.getValue()) {
          XLOG(DBG4)
              << "Blob found in Sapling remote for "
              << request->getRequest<SaplingImportRequest::BlobImport>()->hash;
        }
        stats_->addDuration(
            &SaplingBackingStoreStats::fetchBlob, watch.elapsed());
        continue;
      }

      // The blobs were either not found locally, or, when EdenAPI is enabled,
      // not found on the server. Let's import the blob through the sapling
      // importer.
      // TODO(xavierd): remove when EdenAPI has been rolled out everywhere.
      auto fetchSemiFuture = retryGetBlob(
          request->getRequest<SaplingImportRequest::BlobImport>()->proxyHash);
      futures.emplace_back(
          std::move(fetchSemiFuture)
              .defer([request = std::move(request),
                      watch,
                      stats = stats_.copy()](auto&& result) mutable {
                XLOG(DBG4)
                    << "Imported blob from HgImporter for "
                    << request->getRequest<SaplingImportRequest::BlobImport>()
                           ->hash;
                stats->addDuration(
                    &SaplingBackingStoreStats::fetchBlob, watch.elapsed());
                request
                    ->getPromise<SaplingImportRequest::BlobImport::Response>()
                    ->setTry(std::forward<decltype(result)>(result));
              }));
    }

    folly::collectAll(futures).wait();
  }
}

folly::SemiFuture<BlobPtr> SaplingBackingStore::retryGetBlob(
    HgProxyHash hgInfo) {
  return folly::via(
             retryThreadPool_.get(),
             [this,
              hgInfo = std::move(hgInfo),
              &liveImportBlobWatches = liveImportBlobWatches_] {
               folly::stop_watch<std::chrono::milliseconds> watch;
               RequestMetricsScope queueTracker{&liveImportBlobWatches};

               // NOTE: In the future we plan to update
               // SaplingNativeBackingStore to provide and
               // asynchronous interface enabling us to perform our retries
               // there. In the meantime we use retryThreadPool_ for these
               // longer-running retry requests to avoid starving
               // serverThreadPool_.

               // Flush (and refresh) SaplingNativeBackingStore to ensure all
               // data is written and to rescan pack files or local indexes
               flush();

               // Retry using datapackStore (SaplingNativeBackingStore).
               auto result = folly::makeFuture<BlobPtr>(BlobPtr{nullptr});

               auto fetch_mode =
                   config_->getEdenConfig()->allowRemoteGetBatch.getValue()
                   ? sapling::FetchMode::AllowRemote
                   : sapling::FetchMode::LocalOnly;
               auto blob = getBlobFromBackingStore(hgInfo, fetch_mode);
               if (!blob.hasValue() &&
                   fetch_mode == sapling::FetchMode::LocalOnly) {
                 blob = getBlobFromBackingStore(
                     hgInfo, sapling::FetchMode::RemoteOnly);
               }

               if (blob.hasValue()) {
                 stats_->increment(
                     &SaplingBackingStoreStats::fetchBlobRetrySuccess);
                 result = blob.value();
               } else {
                 // Record miss and return error
                 if (structuredLogger_) {
                   structuredLogger_->logEvent(FetchMiss{
                       store_.getRepoName(),
                       FetchMiss::Blob,
                       blob.exception().what().toStdString(),
                       true});
                 }

                 stats_->increment(
                     &SaplingBackingStoreStats::fetchBlobRetryFailure);
                 auto ew = folly::exception_wrapper{blob.exception()};
                 result = folly::makeFuture<BlobPtr>(std::move(ew));
               }
               stats_->addDuration(
                   &SaplingBackingStoreStats::importBlobDuration,
                   watch.elapsed());
               return result;
             })
      .thenError([this](folly::exception_wrapper&& ew) {
        stats_->increment(&SaplingBackingStoreStats::importBlobError);
        return folly::makeSemiFuture<BlobPtr>(std::move(ew));
      });
}

void SaplingBackingStore::getBlobBatch(
    const ImportRequestsList& importRequests,
    sapling::FetchMode fetchMode) {
  auto preparedRequests =
      prepareRequests<SaplingImportRequest::BlobImport>(importRequests, "Blob");
  auto importRequestsMap = std::move(preparedRequests.first);
  auto requests = std::move(preparedRequests.second);

  store_.getBlobBatch(
      folly::range(requests),
      fetchMode,
      // store_->getBlobBatch is blocking, hence we can take these by reference.
      [&](size_t index, folly::Try<std::unique_ptr<folly::IOBuf>> content) {
        if (content.hasException()) {
          XLOGF(
              DBG6,
              "Failed to import node={} from EdenAPI (batch {}/{}): {}",
              folly::hexlify(requests[index].node),
              index,
              requests.size(),
              content.exception().what().toStdString());
        } else {
          XLOGF(
              DBG6,
              "Imported node={} from EdenAPI (batch: {}/{})",
              folly::hexlify(requests[index].node),
              index,
              requests.size());
        }

        if (content.hasException()) {
          if (structuredLogger_) {
            structuredLogger_->logEvent(FetchMiss{
                store_.getRepoName(),
                FetchMiss::Blob,
                content.exception().what().toStdString(),
                false});
          }

          return;
        }

        const auto& nodeId = requests[index].node;
        XLOGF(DBG9, "Imported Blob node={}", folly::hexlify(nodeId));
        auto& [importRequestList, watch] = importRequestsMap[nodeId];
        auto result = content.hasException()
            ? folly::Try<BlobPtr>{content.exception()}
            : folly::Try{
                  std::make_shared<BlobPtr::element_type>(*content.value())};
        for (auto& importRequest : importRequestList) {
          importRequest->getPromise<BlobPtr>()->setWith(
              [&]() -> folly::Try<BlobPtr> { return result; });
        }

        // Make sure that we're stopping this watch.
        watch.reset();
      });
}

folly::Try<BlobPtr> SaplingBackingStore::getBlobFromBackingStore(
    const HgProxyHash& hgInfo,
    sapling::FetchMode fetchMode) {
  auto blob = store_.getBlob(hgInfo.byteHash(), fetchMode);

  using GetBlobResult = folly::Try<BlobPtr>;

  if (blob.hasValue()) {
    return GetBlobResult{
        std::make_shared<BlobPtr::element_type>(std::move(*blob.value()))};
  } else {
    return GetBlobResult{blob.exception()};
  }
}

void SaplingBackingStore::processTreeImportRequests(
    std::vector<std::shared_ptr<SaplingImportRequest>>&& requests) {
  folly::stop_watch<std::chrono::milliseconds> watch;

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

    XLOGF(DBG4, "Processing tree request for {}", treeImport->hash);
  }

  std::vector<std::shared_ptr<SaplingImportRequest>> retryRequest;
  retryRequest.reserve(requests.size());
  if (config_->getEdenConfig()->allowRemoteGetBatch.getValue()) {
    getTreeBatch(requests, sapling::FetchMode::AllowRemote);
    retryRequest = std::move(requests);
  } else {
    getTreeBatch(requests, sapling::FetchMode::LocalOnly);
    for (auto& request : requests) {
      auto* promise = request->getPromise<TreePtr>();
      if (promise->isFulfilled()) {
        XLOG(DBG4)
            << "Tree found in Sapling local for "
            << request->getRequest<SaplingImportRequest::TreeImport>()->hash;
        stats_->addDuration(
            &SaplingBackingStoreStats::fetchTree, watch.elapsed());
      } else {
        retryRequest.emplace_back(std::move(request));
      }
    }
    getTreeBatch(retryRequest, sapling::FetchMode::RemoteOnly);
  }

  {
    std::vector<folly::SemiFuture<folly::Unit>> futures;
    futures.reserve(retryRequest.size());

    for (auto& request : retryRequest) {
      auto* promise = request->getPromise<TreePtr>();
      if (promise->isFulfilled()) {
        if (!config_->getEdenConfig()->allowRemoteGetBatch.getValue()) {
          XLOG(DBG4)
              << "Tree found in Sapling remote for "
              << request->getRequest<SaplingImportRequest::TreeImport>()->hash;
        }
        stats_->addDuration(
            &SaplingBackingStoreStats::fetchTree, watch.elapsed());
        continue;
      }

      // The trees were either not found locally, or, when EdenAPI is enabled,
      // not found on the server. Let's retry to import the trees
      auto* treeImport =
          request->getRequest<SaplingImportRequest::TreeImport>();
      auto treeSemiFuture =
          retryGetTree(
              treeImport->proxyHash
                  .revHash(), // this is really the manifest node
              treeImport->hash,
              treeImport->proxyHash.path())
              .semi();
      futures.emplace_back(
          std::move(treeSemiFuture)
              .defer([request = std::move(request),
                      watch,
                      stats = stats_.copy()](auto&& result) mutable {
                XLOG(DBG4)
                    << "Imported tree after retry for "
                    << request->getRequest<SaplingImportRequest::TreeImport>()
                           ->hash;
                stats->addDuration(
                    &SaplingBackingStoreStats::fetchTree, watch.elapsed());
                request
                    ->getPromise<SaplingImportRequest::TreeImport::Response>()
                    ->setTry(std::forward<decltype(result)>(result));
              }));
    }

    folly::collectAll(futures).wait();
  }
}

void SaplingBackingStore::getTreeBatch(
    const ImportRequestsList& importRequests,
    sapling::FetchMode fetch_mode) {
  auto preparedRequests =
      prepareRequests<SaplingImportRequest::TreeImport>(importRequests, "Tree");
  auto importRequestsMap = std::move(preparedRequests.first);
  auto requests = std::move(preparedRequests.second);
  auto hgObjectIdFormat = config_->getEdenConfig()->hgObjectIdFormat.getValue();
  const auto filteredPaths =
      config_->getEdenConfig()->hgFilteredPaths.getValue();

  faultInjector_.check("SaplingBackingStore::getTreeBatch", "");
  store_.getTreeBatch(
      folly::range(requests),
      fetch_mode,
      // getTreeBatch is blocking, hence we can take these by
      // reference.
      [&, filteredPaths = filteredPaths](
          size_t index,
          folly::Try<std::shared_ptr<sapling::Tree>> content) mutable {
        if (content.hasException()) {
          XLOGF(
              DBG6,
              "Failed to import node={} from EdenAPI (batch tree {}/{}): {}",
              folly::hexlify(requests[index].node),
              index,
              requests.size(),
              content.exception().what().toStdString());
        } else {
          XLOGF(
              DBG6,
              "Imported node={} from EdenAPI (batch tree: {}/{})",
              folly::hexlify(requests[index].node),
              index,
              requests.size());
        }

        if (content.hasException()) {
          if (structuredLogger_) {
            structuredLogger_->logEvent(FetchMiss{
                store_.getRepoName(),
                FetchMiss::Tree,
                content.exception().what().toStdString(),
                false});
          }

          return;
        }

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
                    treeRequest->hash,
                    treeRequest->proxyHash.path(),
                    hgObjectIdFormat,
                    *filteredPaths,
                    runtimeOptions_->ignoreConfigFilter())};
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
    const std::string& requestType) {
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
          requestType,
          nodeId);
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
                  priorRequest->template getRequest<T>()->hash !=
                  importRequest->getRequest<T>()->hash),
              "{} requests have the same proxyHash (HgProxyHash) but different hash (ObjectId). "
              "This should not happen. Previous request: hash='{}', proxyHash='{}', proxyHash.path='{}'; "
              "current request: hash='{}', proxyHash ='{}', proxyHash.path='{}'.",
              requestType,
              priorRequest->template getRequest<T>()->hash.asHexString(),
              folly::hexlify(
                  priorRequest->template getRequest<T>()->proxyHash.byteHash()),
              priorRequest->template getRequest<T>()->proxyHash.path(),
              importRequest->getRequest<T>()->hash.asHexString(),
              folly::hexlify(
                  importRequest->getRequest<T>()->proxyHash.byteHash()),
              importRequest->getRequest<T>()->proxyHash.path());
        }
      }

      importRequestList.emplace_back(importRequest);
    } else {
      std::vector<std::shared_ptr<SaplingImportRequest>> requests(
          {importRequest});
      importRequestsMap.emplace(
          nodeId, make_pair(requests, &liveBatchedBlobWatches_));
    }
  }

  // Indexable vector of nodeIds - required by SaplingNativeBackingStore API.
  // With the current implementation, we can't efficiently deduplicate the
  // requests only based on nodeId since multiple requests for the same nodeId
  // can have different FetchCauses, which might trigger different behaviors in
  // the backingstore.
  std::vector<sapling::SaplingRequest> requests;
  for (const auto& importRequestsIdPair : importRequestsMap) {
    // Deduplicate the requests for a given nodeId based on the FetchCause.
    std::set<ObjectFetchContext::Cause> seenCausesForId;
    const ImportRequestsList& importRequestsForId =
        importRequestsIdPair.second.first;
    for (const auto& request : importRequestsForId) {
      if (request &&
          (seenCausesForId.find(request->getCause()) ==
           seenCausesForId.end())) {
        requests.push_back(sapling::SaplingRequest{
            importRequestsIdPair.first, request->getCause()});
        // Mark this cause as seen
        seenCausesForId.insert(request->getCause());
      }
    }
  }

  return std::make_pair(std::move(importRequestsMap), std::move(requests));
}

void SaplingBackingStore::processBlobMetaImportRequests(
    std::vector<std::shared_ptr<SaplingImportRequest>>&& requests) {
  folly::stop_watch<std::chrono::milliseconds> watch;

  for (auto& request : requests) {
    auto* blobMetaImport =
        request->getRequest<SaplingImportRequest::BlobMetaImport>();

    // TODO: We could reduce the number of lock acquisitions by adding a batch
    // publish method.
    traceBus_->publish(HgImportTraceEvent::start(
        request->getUnique(),
        HgImportTraceEvent::BLOBMETA,
        blobMetaImport->proxyHash,
        request->getPriority().getClass(),
        request->getCause(),
        request->getPid()));

    XLOGF(DBG4, "Processing blob meta request for {}", blobMetaImport->hash);
  }

  std::vector<std::shared_ptr<SaplingImportRequest>> retryRequest;
  retryRequest.reserve(requests.size());
  if (config_->getEdenConfig()->allowRemoteGetBatch.getValue()) {
    getBlobMetadataBatch(requests, sapling::FetchMode::AllowRemote);
    retryRequest = std::move(requests);
  } else {
    getBlobMetadataBatch(requests, sapling::FetchMode::LocalOnly);
    for (auto& request : requests) {
      auto* promise = request->getPromise<BlobMetadataPtr>();
      if (promise->isFulfilled()) {
        XLOG(DBG4) << "BlobMetaData found in Sapling local for "
                   << request
                          ->getRequest<SaplingImportRequest::BlobMetaImport>()
                          ->hash;
        stats_->addDuration(
            &SaplingBackingStoreStats::fetchBlobMetadata, watch.elapsed());

      } else {
        retryRequest.emplace_back(std::move(request));
      }
    }
    getBlobMetadataBatch(retryRequest, sapling::FetchMode::RemoteOnly);
  }

  {
    for (auto& request : retryRequest) {
      auto* promise = request->getPromise<BlobMetadataPtr>();
      if (promise->isFulfilled()) {
        if (!config_->getEdenConfig()->allowRemoteGetBatch.getValue()) {
          XLOG(DBG4) << "BlobMetaData found in Sapling remote for "
                     << request
                            ->getRequest<SaplingImportRequest::BlobMetaImport>()
                            ->hash;
        }
        stats_->addDuration(
            &SaplingBackingStoreStats::fetchBlobMetadata, watch.elapsed());
        continue;
      }

      // The code waiting on the promise will fallback to fetching the Blob to
      // compute the blob metadata. We can't trigger a blob fetch here without
      // the risk of running into a deadlock: if all import thread are in this
      // code path, there are no free importer to fetch blobs.
      promise->setValue(nullptr);
    }
  }
}

void SaplingBackingStore::getBlobMetadataBatch(
    const ImportRequestsList& importRequests,
    sapling::FetchMode fetch_mode) {
  auto preparedRequests = prepareRequests<SaplingImportRequest::BlobMetaImport>(
      importRequests, "BlobMetadata");
  auto importRequestsMap = std::move(preparedRequests.first);
  auto requests = std::move(preparedRequests.second);

  store_.getBlobMetadataBatch(
      folly::range(requests),
      fetch_mode,
      // store_.getBlobMetadataBatch is blocking, hence we can take these by
      // reference.
      [&](size_t index,
          folly::Try<std::shared_ptr<sapling::FileAuxData>> auxTry) {
        if (auxTry.hasException()) {
          XLOGF(
              DBG6,
              "Failed to import metadata node={} from EdenAPI (batch {}/{}): {}",
              folly::hexlify(requests[index].node),
              index,
              requests.size(),
              auxTry.exception().what().toStdString());
        } else {
          XLOGF(
              DBG6,
              "Imported metadata node={} from EdenAPI (batch: {}/{})",
              folly::hexlify(requests[index].node),
              index,
              requests.size());
        }

        if (auxTry.hasException()) {
          if (structuredLogger_) {
            structuredLogger_->logEvent(FetchMiss{
                store_.getRepoName(),
                FetchMiss::BlobMetadata,
                auxTry.exception().what().toStdString(),
                false});
          }

          return;
        }

        const auto& nodeId = requests[index].node;
        XLOGF(DBG9, "Imported BlobMetadata={}", folly::hexlify(nodeId));
        auto& [importRequestList, watch] = importRequestsMap[nodeId];
        folly::Try<BlobMetadataPtr> result;
        if (auxTry.hasException()) {
          result = folly::Try<BlobMetadataPtr>{auxTry.exception()};
        } else {
          auto& aux = auxTry.value();
          std::optional<Hash32> blake3;
          if (aux->has_blake3) {
            blake3.emplace(Hash32{std::move(aux->content_blake3)});
          }

          result = folly::Try{std::make_shared<BlobMetadataPtr::element_type>(
              Hash20{std::move(aux->content_sha1)}, blake3, aux->total_size)};
        }
        for (auto& importRequest : importRequestList) {
          importRequest->getPromise<BlobMetadataPtr>()->setWith(
              [&]() -> folly::Try<BlobMetadataPtr> { return result; });
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
    } else if (first->isType<SaplingImportRequest::BlobMetaImport>()) {
      processBlobMetaImportRequests(std::move(requests));
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
  // Mercurial's blob hashes also include history metadata, so there may be
  // multiple different blob hashes for the same file contents.
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

  if (auto tree = getTreeLocal(id, proxyHash)) {
    XLOG(DBG5) << "imported tree of '" << proxyHash.path() << "', "
               << proxyHash.revHash().toString() << " from hgcache";
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
    auto request = SaplingImportRequest::makeTreeImportRequest(
        id,
        proxyHash,
        context->getPriority(),
        context->getCause(),
        context->getClientPid());
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
              context->getClientPid()));
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
    const HgProxyHash& proxyHash) {
  auto tree =
      store_.getTree(proxyHash.byteHash(), sapling::FetchMode::LocalOnly);
  if (tree.hasValue()) {
    auto hgObjectIdFormat =
        config_->getEdenConfig()->hgObjectIdFormat.getValue();
    const auto filteredPaths =
        config_->getEdenConfig()->hgFilteredPaths.getValue();
    return fromRawTree(
        tree.value().get(),
        edenTreeId,
        proxyHash.path(),
        hgObjectIdFormat,
        *filteredPaths,
        runtimeOptions_->ignoreConfigFilter());
  }

  return nullptr;
}

folly::Try<TreePtr> SaplingBackingStore::getTreeRemote(
    const RelativePath& path,
    const Hash20& manifestId,
    const ObjectId& edenTreeId,
    const ObjectFetchContextPtr& /*context*/) {
  auto tree = store_.getTree(
      manifestId.getBytes(),
      sapling::FetchMode::RemoteOnly /*, sapling::ClientRequestInfo(context)*/);

  using GetTreeResult = folly::Try<TreePtr>;

  if (tree.hasValue()) {
    auto hgObjectIdFormat =
        config_->getEdenConfig()->hgObjectIdFormat.getValue();
    const auto filteredPaths =
        config_->getEdenConfig()->hgFilteredPaths.getValue();
    return GetTreeResult{fromRawTree(
        tree.value().get(),
        edenTreeId,
        path,
        std::move(hgObjectIdFormat),
        std::move(*filteredPaths),
        runtimeOptions_->ignoreConfigFilter())};
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

  auto blob = getBlobLocal(proxyHash);
  if (blob.hasValue()) {
    return folly::makeSemiFuture(GetBlobResult{
        std::move(blob.value()), ObjectFetchContext::Origin::FromDiskCache});
  }

  return getBlobImpl(id, proxyHash, context)
      .ensure([scope = std::move(scope)] {})
      .semi();
}

ImmediateFuture<BackingStore::GetBlobResult> SaplingBackingStore::getBlobImpl(
    const ObjectId& id,
    const HgProxyHash& proxyHash,
    const ObjectFetchContextPtr& context) {
  auto getBlobFuture = makeImmediateFutureWith([&] {
    XLOG(DBG4) << "make blob import request for " << proxyHash.path()
               << ", hash is:" << id;

    auto request = SaplingImportRequest::makeBlobImportRequest(
        id,
        proxyHash,
        context->getPriority(),
        context->getCause(),
        context->getClientPid());
    auto unique = request->getUnique();

    auto importTracker =
        std::make_unique<RequestMetricsScope>(&pendingImportBlobWatches_);
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
              context->getClientPid()));
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

folly::SemiFuture<BackingStore::GetBlobMetaResult>
SaplingBackingStore::getBlobMetadata(
    const ObjectId& id,
    const ObjectFetchContextPtr& context) {
  DurationScope<EdenStats> scope{
      stats_, &SaplingBackingStoreStats::getBlobMetadata};

  HgProxyHash proxyHash;
  try {
    proxyHash =
        HgProxyHash::load(localStore_.get(), id, "getBlobMetadata", *stats_);
  } catch (const std::exception&) {
    logMissingProxyHash();
    throw;
  }

  logBackingStoreFetch(
      *context,
      folly::Range{&proxyHash, 1},
      ObjectFetchContext::ObjectType::BlobMetadata);

  auto metadata = getLocalBlobMetadata(proxyHash);
  if (metadata.hasValue()) {
    return folly::makeSemiFuture(GetBlobMetaResult{
        std::move(metadata.value()),
        ObjectFetchContext::Origin::FromDiskCache});
  }

  return getBlobMetadataImpl(id, proxyHash, context)
      .ensure([scope = std::move(scope)] {})
      .semi();
}

ImmediateFuture<BackingStore::GetBlobMetaResult>
SaplingBackingStore::getBlobMetadataImpl(
    const ObjectId& id,
    const HgProxyHash& proxyHash,
    const ObjectFetchContextPtr& context) {
  if (!config_->getEdenConfig()->fetchHgAuxMetadata.getValue()) {
    return BackingStore::GetBlobMetaResult{
        nullptr, ObjectFetchContext::Origin::NotFetched};
  }

  auto getBlobMetaFuture = makeImmediateFutureWith([&] {
    XLOG(DBG4) << "make blob meta import request for " << proxyHash.path()
               << ", hash is:" << id;

    auto request = SaplingImportRequest::makeBlobMetaImportRequest(
        id,
        proxyHash,
        context->getPriority(),
        context->getCause(),
        context->getClientPid());
    auto unique = request->getUnique();

    auto importTracker =
        std::make_unique<RequestMetricsScope>(&pendingImportBlobMetaWatches_);
    traceBus_->publish(HgImportTraceEvent::queue(
        unique,
        HgImportTraceEvent::BLOBMETA,
        proxyHash,
        context->getPriority().getClass(),
        context->getCause(),
        context->getClientPid()));

    return queue_.enqueueBlobMeta(std::move(request))
        .ensure([this,
                 unique,
                 proxyHash,
                 context = context.copy(),
                 importTracker = std::move(importTracker)]() {
          traceBus_->publish(HgImportTraceEvent::finish(
              unique,
              HgImportTraceEvent::BLOBMETA,
              proxyHash,
              context->getPriority().getClass(),
              context->getCause(),
              context->getClientPid()));
        });
  });

  return std::move(getBlobMetaFuture)
      .thenTry([this, id](folly::Try<BlobMetadataPtr>&& result) {
        this->queue_.markImportAsFinished<BlobMetadataPtr::element_type>(
            id, result);
        auto blobMeta = std::move(result).value();
        return GetBlobMetaResult{
            std::move(blobMeta), ObjectFetchContext::Origin::FromNetworkFetch};
      });
}

folly::Try<BlobMetadataPtr> SaplingBackingStore::getLocalBlobMetadata(
    const HgProxyHash& hgInfo) {
  auto metadata =
      store_.getBlobMetadata(hgInfo.byteHash(), true /*local_only*/);

  using GetBlobMetadataResult = folly::Try<BlobMetadataPtr>;

  if (metadata.hasValue()) {
    std::optional<Hash32> blake3;
    if (metadata.value()->has_blake3) {
      blake3.emplace(Hash32{std::move(metadata.value()->content_blake3)});
    }
    return GetBlobMetadataResult{
        std::make_shared<BlobMetadataPtr::element_type>(BlobMetadata{
            Hash20{std::move(metadata.value()->content_sha1)},
            blake3,
            metadata.value()->total_size})};
  } else {
    return GetBlobMetadataResult{metadata.exception()};
  }
}

ImmediateFuture<BackingStore::GetRootTreeResult>
SaplingBackingStore::getRootTree(
    const RootId& rootId,
    const ObjectFetchContextPtr& context) {
  ObjectId commitId = hashFromRootId(rootId);

  return localStore_
      ->getImmediateFuture(KeySpace::HgCommitToTreeFamily, commitId)
      .thenValue(
          [this, commitId, context = context.copy()](StoreResult result)
              -> folly::SemiFuture<BackingStore::GetRootTreeResult> {
            if (!result.isValid()) {
              return importTreeManifest(commitId, context)
                  .thenValue([this, commitId](TreePtr rootTree) {
                    XLOG(DBG1) << "imported mercurial commit " << commitId
                               << " as tree " << rootTree->getHash();

                    localStore_->put(
                        KeySpace::HgCommitToTreeFamily,
                        commitId,
                        rootTree->getHash().getBytes());
                    return BackingStore::GetRootTreeResult{
                        rootTree, rootTree->getHash()};
                  });
            }

            auto rootTreeHash = HgProxyHash::load(
                localStore_.get(),
                ObjectId{result.bytes()},
                "getRootTree",
                *stats_);
            return importTreeManifestImpl(rootTreeHash.revHash(), context)
                .thenValue([](TreePtr tree) {
                  return BackingStore::GetRootTreeResult{tree, tree->getHash()};
                });
          });
}

folly::Future<TreePtr> SaplingBackingStore::importTreeManifest(
    const ObjectId& commitId,
    const ObjectFetchContextPtr& context) {
  return folly::via(
             serverThreadPool_,
             [this, commitId] { return getManifestNode(commitId); })
      .thenValue([this, commitId, fetchContext = context.copy()](
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
        return importTreeManifestImpl(*std::move(manifestNode), fetchContext);
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
    const ObjectFetchContextPtr& context) {
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
  folly::stop_watch<std::chrono::milliseconds> watch;
  auto tree =
      getTreeFromBackingStore(path.copy(), manifestNode, objectId, context);
  if (tree.hasValue()) {
    XLOG(DBG4) << "imported tree node=" << manifestNode << " path=" << path
               << " from SaplingNativeBackingStore";
    stats_->addDuration(&SaplingBackingStoreStats::fetchTree, watch.elapsed());
    return folly::makeFuture(std::move(tree.value()));
  }
  // retry once if the initial fetch failed
  return retryGetTree(manifestNode, objectId, path);
}

folly::Future<TreePtr> SaplingBackingStore::retryGetTree(
    const Hash20& manifestNode,
    const ObjectId& edenTreeID,
    RelativePathPiece path) {
  XLOG(DBG6) << "importing tree " << edenTreeID << ": hg manifest "
             << manifestNode << " for path \"" << path << "\"";

  // Explicitly check for the null ID on the root directory.
  // This isn't actually present in the mercurial data store; it has to be
  // handled specially in the code.
  if (path.empty() && manifestNode == kZeroHash) {
    auto tree = std::make_shared<TreePtr::element_type>(
        Tree::container{kPathMapDefaultCaseSensitive}, edenTreeID);
    return folly::makeFuture(std::move(tree));
  }

  if (!FLAGS_hg_fetch_missing_trees) {
    auto ew = folly::exception_wrapper{std::runtime_error{
        "Data not available via edenapi, skipping fallback to importer because "
        "of FLAGS_hg_fetch_missing_trees"}};
    return folly::makeFuture<TreePtr>(std::move(ew));
  }

  folly::stop_watch<std::chrono::milliseconds> watch;
  auto writeBatch = localStore_->beginWrite();
  // When aux metadata is enabled hg fetches file metadata along with get tree
  // request, no need for separate network call!
  return retryGetTreeImpl(
             manifestNode, edenTreeID, path.copy(), std::move(writeBatch))
      .thenValue([this, watch, config = config_](TreePtr&& result) mutable {
        stats_->addDuration(
            &SaplingBackingStoreStats::fetchTree, watch.elapsed());
        return std::move(result);
      });
}

folly::Try<TreePtr> SaplingBackingStore::getTreeFromBackingStore(
    const RelativePath& path,
    const Hash20& manifestId,
    const ObjectId& edenTreeId,
    const ObjectFetchContextPtr& /*context*/) {
  folly::Try<std::shared_ptr<sapling::Tree>> tree;
  if (config_->getEdenConfig()->allowRemoteGetBatch.getValue()) {
    // For root trees we will try getting the tree locally first.  This allows
    // us to catch when Mercurial might have just written a tree to the store,
    // and refresh the store so that the store can pick it up.  We don't do
    // this for all trees, as it would cause a lot of additional work on every
    // cache miss, and just doing it for root trees is sufficient to detect the
    // scenario where Mercurial just wrote a brand new tree.
    sapling::FetchMode fetchMode = sapling::FetchMode::AllowRemote;
    if (path.empty()) {
      fetchMode = sapling::FetchMode::LocalOnly;
    }
    tree = store_.getTree(manifestId.getBytes(), fetchMode);
    if (tree.hasException() && fetchMode == sapling::FetchMode::LocalOnly) {
      // Mercurial might have just written the tree to the store. Refresh the
      // store and try again, this time allowing remote fetches.
      store_.flush();
      tree = store_.getTree(
          manifestId.getBytes(), sapling::FetchMode::AllowRemote);
    }
  } else {
    tree = store_.getTree(manifestId.getBytes(), sapling::FetchMode::LocalOnly);
    if (tree.hasException()) {
      if (path.empty()) {
        // This allows us to catch when Mercurial might have just written a tree
        // to the store, and refresh the store so that the store can pick it up.
        // We don't do this for all trees, as it would cause a lot of additional
        // work on every cache miss, and just doing it for root trees is
        // sufficient to detect the scenario where Mercurial just wrote a brand
        // new tree.
        store_.flush();
      }
      tree =
          store_.getTree(manifestId.getBytes(), sapling::FetchMode::RemoteOnly);
    }
  }

  using GetTreeResult = folly::Try<TreePtr>;

  if (tree.hasValue()) {
    auto hgObjectIdFormat =
        config_->getEdenConfig()->hgObjectIdFormat.getValue();
    const auto filteredPaths =
        config_->getEdenConfig()->hgFilteredPaths.getValue();
    return GetTreeResult{fromRawTree(
        tree.value().get(),
        edenTreeId,
        path,
        std::move(hgObjectIdFormat),
        std::move(*filteredPaths),
        runtimeOptions_->ignoreConfigFilter())};
  } else {
    return GetTreeResult{tree.exception()};
  }
}

folly::Future<TreePtr> SaplingBackingStore::retryGetTreeImpl(
    Hash20 manifestNode,
    ObjectId edenTreeID,
    RelativePath path,
    std::shared_ptr<LocalStore::WriteBatch> writeBatch) {
  return folly::via(
             retryThreadPool_.get(),
             [this,
              path,
              manifestNode,
              edenTreeID,
              writeBatch,
              &liveImportTreeWatches = liveImportTreeWatches_] {
               folly::stop_watch<std::chrono::milliseconds> watch;
               RequestMetricsScope queueTracker{&liveImportTreeWatches};

               // NOTE: In the future we plan to update
               // SaplingNativeBackingStore to provide and
               // asynchronous interface enabling us to perform our retries
               // there. In the meantime we use retryThreadPool_ for these
               // longer-running retry requests to avoid starving
               // serverThreadPool_.

               // Flush (and refresh) SaplingNativeBackingStore to ensure all
               // data is written and to rescan pack files or local indexes
               flush();

               // Retry using datapackStore (SaplingNativeBackingStore)
               auto result = folly::makeFuture<TreePtr>(TreePtr{nullptr});
               auto tree = getTreeFromBackingStore(
                   path, manifestNode, edenTreeID, /*context*/ nullptr);
               if (tree.hasValue()) {
                 stats_->increment(
                     &SaplingBackingStoreStats::fetchTreeRetrySuccess);
                 result = tree.value();
               } else {
                 // Record miss and return error
                 if (structuredLogger_) {
                   structuredLogger_->logEvent(FetchMiss{
                       store_.getRepoName(),
                       FetchMiss::Tree,
                       tree.exception().what().toStdString(),
                       true});
                 }

                 stats_->increment(
                     &SaplingBackingStoreStats::fetchTreeRetryFailure);
                 auto ew = folly::exception_wrapper{tree.exception()};
                 result = folly::makeFuture<TreePtr>(std::move(ew));
               }
               stats_->addDuration(
                   &SaplingBackingStoreStats::importTreeDuration,
                   watch.elapsed());
               return result;
             })
      .thenError([this](folly::exception_wrapper&& ew) {
        stats_->increment(&SaplingBackingStoreStats::importTreeError);
        return folly::makeFuture<TreePtr>(std::move(ew));
      });
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
        // already present locally, so the check for local blob is pure overhead
        // when prefetching.
        std::vector<ImmediateFuture<GetBlobResult>> futures;
        futures.reserve(ids.size());

        for (size_t i = 0; i < ids.size(); i++) {
          const auto& id = ids[i];
          const auto& proxyHash = proxyHashes[i];

          futures.emplace_back(getBlobImpl(id, proxyHash, context));
        }

        return collectAllSafe(std::move(futures)).unit();
      })
      .semi();
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
    case SaplingImportObject::BLOBMETA:
    case SaplingImportObject::BATCHED_BLOBMETA:
      return pendingImportBlobMetaWatches_;
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
    case SaplingImportObject::BLOBMETA:
      return liveImportBlobMetaWatches_;
    case SaplingImportObject::PREFETCH:
      return liveImportPrefetchWatches_;
    case SaplingImportObject::BATCHED_BLOB:
      return liveBatchedBlobWatches_;
    case SaplingImportObject::BATCHED_TREE:
      return liveBatchedTreeWatches_;
    case SaplingImportObject::BATCHED_BLOBMETA:
      return liveBatchedBlobMetaWatches_;
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
    case SaplingImportObject::BLOBMETA:
      return "blobmeta";
    case SaplingImportObject::BATCHED_BLOB:
      return "batched_blob";
    case SaplingImportObject::BATCHED_TREE:
      return "batched_tree";
    case SaplingImportObject::BATCHED_BLOBMETA:
      return "batched_blobmeta";
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
  // metadata is only available remotely, there will be no metadata available
  // to prefetch.
  //
  // When the local store is populated with metadata for newly-created
  // manifests then we can update this so that is true when appropriate.
  /**
   * Import the root manifest for the specied revision using mercurial
   * treemanifest data.  This is called when the root manifest is provided
   * to EdenFS directly by the hg client.
   */
  auto commitId = hashFromRootId(rootId);
  return localStore_
      ->getImmediateFuture(KeySpace::HgCommitToTreeFamily, commitId)
      .thenValue(
          [this, commitId, manifestId, context = context.copy()](
              StoreResult result) -> folly::Future<folly::Unit> {
            if (result.isValid()) {
              // We have already imported this commit, nothing to do.
              return folly::unit;
            }

            return importTreeManifestImpl(manifestId, context)
                .thenValue([this, commitId, manifestId](TreePtr rootTree) {
                  XLOG(DBG3) << "imported mercurial commit " << commitId
                             << " with manifest " << manifestId << " as tree "
                             << rootTree->getHash();

                  localStore_->put(
                      KeySpace::HgCommitToTreeFamily,
                      commitId,
                      rootTree->getHash().getBytes());
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
