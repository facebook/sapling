/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/hg/HgQueuedBackingStore.h"

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

#include "eden/common/utils/Bug.h"
#include "eden/common/utils/EnumValue.h"
#include "eden/common/utils/IDGen.h"
#include "eden/common/utils/PathFuncs.h"
#include "eden/common/utils/Throw.h"
#include "eden/common/utils/UnboundedQueueExecutor.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/service/ThriftUtil.h"
#include "eden/fs/store/BackingStoreLogger.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/store/StoreResult.h"
#include "eden/fs/store/hg/HgImportRequest.h"
#include "eden/fs/store/hg/HgProxyHash.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/RequestMetricsScope.h"
#include "eden/fs/telemetry/StructuredLogger.h"
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
    "the number of hg import threads per repo");

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

HgQueuedBackingStore::HgQueuedBackingStore(
    AbsolutePathPiece repository,
    std::shared_ptr<LocalStore> localStore,
    EdenStatsPtr stats,
    UnboundedQueueExecutor* serverThreadPool,
    std::shared_ptr<ReloadableConfig> config,
    std::unique_ptr<HgBackingStoreOptions> runtimeOptions,
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
      activityBuffer_{
          config_->getEdenConfig()->hgActivityBufferSize.getValue()},
      traceBus_{TraceBus<HgImportTraceEvent>::create(
          "hg",
          config_->getEdenConfig()->HgTraceBusCapacity.getValue())},
      datapackStore_{std::make_unique<HgDatapackStore>(
          repository,
          HgDatapackStore::computeSaplingOptions(),
          HgDatapackStore::computeRuntimeOptions(std::move(runtimeOptions)),
          config_,
          structuredLogger_,
          faultInjector)} {
  uint8_t numberThreads =
      config_->getEdenConfig()->numBackingstoreThreads.getValue();
  if (!numberThreads) {
    XLOG(WARN)
        << "HgQueuedBackingStore configured to use 0 threads. Invalid, using one thread instead";
    numberThreads = 1;
  }
  threads_.reserve(numberThreads);
  for (uint16_t i = 0; i < numberThreads; i++) {
    threads_.emplace_back(&HgQueuedBackingStore::processRequest, this);
  }

  hgTraceHandle_ = traceBus_->subscribeFunction(
      folly::to<std::string>("hg-activitybuffer-", getRepoName().value_or("")),
      [this](const HgImportTraceEvent& event) {
        activityBuffer_.addEvent(event);
      });
}

/**
 * Create an HgQueuedBackingStore suitable for use in unit tests. It uses an
 * inline executor to process loaded objects rather than the thread pools used
 * in production Eden.
 */
HgQueuedBackingStore::HgQueuedBackingStore(
    AbsolutePathPiece repository,
    std::shared_ptr<LocalStore> localStore,
    EdenStatsPtr stats,
    std::shared_ptr<ReloadableConfig> config,
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
      activityBuffer_{
          config_->getEdenConfig()->hgActivityBufferSize.getValue()},
      traceBus_{TraceBus<HgImportTraceEvent>::create(
          "hg",
          config_->getEdenConfig()->HgTraceBusCapacity.getValue())},
      datapackStore_{std::make_unique<HgDatapackStore>(
          repository,
          HgDatapackStore::computeTestSaplingOptions(),
          HgDatapackStore::computeTestRuntimeOptions(
              std::make_unique<HgBackingStoreOptions>(
                  /*ignoreFilteredPathsConfig=*/false)),
          config_,
          nullptr,
          faultInjector)} {
  uint8_t numberThreads =
      config_->getEdenConfig()->numBackingstoreThreads.getValue();
  if (!numberThreads) {
    XLOG(WARN)
        << "HgQueuedBackingStore configured to use 0 threads. Invalid, using one thread instead";
    numberThreads = 1;
  }
  threads_.reserve(numberThreads);
  for (uint16_t i = 0; i < numberThreads; i++) {
    threads_.emplace_back(&HgQueuedBackingStore::processRequest, this);
  }

  hgTraceHandle_ = traceBus_->subscribeFunction(
      folly::to<std::string>("hg-activitybuffer-", getRepoName().value_or("")),
      [this](const HgImportTraceEvent& event) {
        activityBuffer_.addEvent(event);
      });
}

HgQueuedBackingStore::~HgQueuedBackingStore() {
  queue_.stop();
  for (auto& thread : threads_) {
    thread.join();
  }
}

void HgQueuedBackingStore::processBlobImportRequests(
    std::vector<std::shared_ptr<HgImportRequest>>&& requests) {
  folly::stop_watch<std::chrono::milliseconds> watch;

  XLOG(DBG4) << "Processing blob import batch size=" << requests.size();

  for (auto& request : requests) {
    auto* blobImport = request->getRequest<HgImportRequest::BlobImport>();

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

  datapackStore_->getBlobBatch(requests);

  {
    std::vector<folly::SemiFuture<folly::Unit>> futures;
    futures.reserve(requests.size());

    for (auto& request : requests) {
      auto* promise = request->getPromise<BlobPtr>();
      if (promise->isFulfilled()) {
        stats_->addDuration(&HgBackingStoreStats::fetchBlob, watch.elapsed());
        continue;
      }

      // The blobs were either not found locally, or, when EdenAPI is enabled,
      // not found on the server. Let's import the blob through the hg importer.
      // TODO(xavierd): remove when EdenAPI has been rolled out everywhere.
      auto fetchSemiFuture = retryGetBlob(
          request->getRequest<HgImportRequest::BlobImport>()->proxyHash);
      futures.emplace_back(
          std::move(fetchSemiFuture)
              .defer([request = std::move(request),
                      watch,
                      stats = stats_.copy()](auto&& result) mutable {
                XLOG(DBG4)
                    << "Imported blob from HgImporter for "
                    << request->getRequest<HgImportRequest::BlobImport>()->hash;
                stats->addDuration(
                    &HgBackingStoreStats::fetchBlob, watch.elapsed());
                request->getPromise<HgImportRequest::BlobImport::Response>()
                    ->setTry(std::forward<decltype(result)>(result));
              }));
    }

    folly::collectAll(futures).wait();
  }
}

folly::SemiFuture<BlobPtr> HgQueuedBackingStore::retryGetBlob(
    HgProxyHash hgInfo) {
  return folly::via(
             retryThreadPool_.get(),
             [this,
              hgInfo = std::move(hgInfo),
              &liveImportBlobWatches = liveImportBlobWatches_] {
               folly::stop_watch<std::chrono::milliseconds> watch;
               RequestMetricsScope queueTracker{&liveImportBlobWatches};

               // NOTE: In the future we plan to update
               // SaplingNativeBackingStore (and HgDatapackStore) to provide and
               // asynchronous interface enabling us to perform our retries
               // there. In the meantime we use retryThreadPool_ for these
               // longer-running retry requests to avoid starving
               // serverThreadPool_.

               // Flush (and refresh) SaplingNativeBackingStore to ensure all
               // data is written and to rescan pack files or local indexes
               datapackStore_->flush();

               // Retry using datapackStore (SaplingNativeBackingStore).
               auto result = folly::makeFuture<BlobPtr>(BlobPtr{nullptr});
               auto blob = datapackStore_->getBlob(
                   hgInfo, sapling::FetchMode::AllowRemote);
               if (blob.hasValue()) {
                 stats_->increment(&HgBackingStoreStats::fetchBlobRetrySuccess);
                 result = blob.value();
               } else {
                 // Record miss and return error
                 if (structuredLogger_) {
                   structuredLogger_->logEvent(FetchMiss{
                       datapackStore_->getRepoName(),
                       FetchMiss::Blob,
                       blob.exception().what().toStdString(),
                       true});
                 }

                 stats_->increment(&HgBackingStoreStats::fetchBlobRetryFailure);
                 auto ew = folly::exception_wrapper{blob.exception()};
                 result = folly::makeFuture<BlobPtr>(std::move(ew));
               }
               stats_->addDuration(
                   &HgBackingStoreStats::importBlobDuration, watch.elapsed());
               return result;
             })
      .thenError([this](folly::exception_wrapper&& ew) {
        stats_->increment(&HgBackingStoreStats::importBlobError);
        return folly::makeSemiFuture<BlobPtr>(std::move(ew));
      });
}

void HgQueuedBackingStore::processTreeImportRequests(
    std::vector<std::shared_ptr<HgImportRequest>>&& requests) {
  folly::stop_watch<std::chrono::milliseconds> watch;

  for (auto& request : requests) {
    auto* treeImport = request->getRequest<HgImportRequest::TreeImport>();

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

  datapackStore_->getTreeBatch(requests);

  {
    std::vector<folly::SemiFuture<folly::Unit>> futures;
    futures.reserve(requests.size());

    for (auto& request : requests) {
      auto* promise = request->getPromise<TreePtr>();
      if (promise->isFulfilled()) {
        stats_->addDuration(&HgBackingStoreStats::fetchTree, watch.elapsed());
        continue;
      }

      // The trees were either not found locally, or, when EdenAPI is enabled,
      // not found on the server. Let's retry to import the trees
      auto* treeImport = request->getRequest<HgImportRequest::TreeImport>();
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
                    << "Imported tree from HgImporter for "
                    << request->getRequest<HgImportRequest::TreeImport>()->hash;
                stats->addDuration(
                    &HgBackingStoreStats::fetchTree, watch.elapsed());
                request->getPromise<HgImportRequest::TreeImport::Response>()
                    ->setTry(std::forward<decltype(result)>(result));
              }));
    }

    folly::collectAll(futures).wait();
  }
}

void HgQueuedBackingStore::processBlobMetaImportRequests(
    std::vector<std::shared_ptr<HgImportRequest>>&& requests) {
  folly::stop_watch<std::chrono::milliseconds> watch;

  for (auto& request : requests) {
    auto* blobMetaImport =
        request->getRequest<HgImportRequest::BlobMetaImport>();

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

  datapackStore_->getBlobMetadataBatch(requests);

  {
    for (auto& request : requests) {
      auto* promise = request->getPromise<BlobMetadataPtr>();
      if (promise->isFulfilled()) {
        stats_->addDuration(
            &HgBackingStoreStats::fetchBlobMetadata, watch.elapsed());
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

void HgQueuedBackingStore::processRequest() {
  folly::setThreadName("hgqueue");
  for (;;) {
    auto requests = queue_.dequeue();

    if (requests.empty()) {
      break;
    }

    const auto& first = requests.at(0);

    if (first->isType<HgImportRequest::BlobImport>()) {
      processBlobImportRequests(std::move(requests));
    } else if (first->isType<HgImportRequest::TreeImport>()) {
      processTreeImportRequests(std::move(requests));
    } else if (first->isType<HgImportRequest::BlobMetaImport>()) {
      processBlobMetaImportRequests(std::move(requests));
    }
  }
}

ObjectComparison HgQueuedBackingStore::compareObjectsById(
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

RootId HgQueuedBackingStore::parseRootId(folly::StringPiece rootId) {
  // rootId can be 20-byte binary or 40-byte hex. Canonicalize, unconditionally
  // returning 40-byte hex.
  return RootId{hash20FromThrift(rootId).toString()};
}

std::string HgQueuedBackingStore::renderRootId(const RootId& rootId) {
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

ObjectId HgQueuedBackingStore::staticParseObjectId(
    folly::StringPiece objectId) {
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

std::string HgQueuedBackingStore::staticRenderObjectId(
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

folly::SemiFuture<BackingStore::GetTreeResult> HgQueuedBackingStore::getTree(
    const ObjectId& id,
    const ObjectFetchContextPtr& context) {
  DurationScope scope{stats_, &HgBackingStoreStats::getTree};

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

  if (auto tree = datapackStore_->getTreeLocal(id, proxyHash)) {
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
HgQueuedBackingStore::getTreeEnqueue(
    const ObjectId& id,
    const HgProxyHash& proxyHash,
    const ObjectFetchContextPtr& context) {
  auto getTreeFuture = makeImmediateFutureWith([&] {
    auto request = HgImportRequest::makeTreeImportRequest(
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

folly::SemiFuture<BackingStore::GetBlobResult> HgQueuedBackingStore::getBlob(
    const ObjectId& id,
    const ObjectFetchContextPtr& context) {
  DurationScope scope{stats_, &HgBackingStoreStats::getBlob};

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

  auto blob = datapackStore_->getBlobLocal(proxyHash);
  if (blob.hasValue()) {
    return folly::makeSemiFuture(GetBlobResult{
        std::move(blob.value()), ObjectFetchContext::Origin::FromDiskCache});
  }

  return getBlobImpl(id, proxyHash, context)
      .ensure([scope = std::move(scope)] {})
      .semi();
}

ImmediateFuture<BackingStore::GetBlobResult> HgQueuedBackingStore::getBlobImpl(
    const ObjectId& id,
    const HgProxyHash& proxyHash,
    const ObjectFetchContextPtr& context) {
  auto getBlobFuture = makeImmediateFutureWith([&] {
    XLOG(DBG4) << "make blob import request for " << proxyHash.path()
               << ", hash is:" << id;

    auto request = HgImportRequest::makeBlobImportRequest(
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
HgQueuedBackingStore::getBlobMetadata(
    const ObjectId& id,
    const ObjectFetchContextPtr& context) {
  DurationScope scope{stats_, &HgBackingStoreStats::getBlobMetadata};

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

  auto metadata = datapackStore_->getLocalBlobMetadata(proxyHash);
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
HgQueuedBackingStore::getBlobMetadataImpl(
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

    auto request = HgImportRequest::makeBlobMetaImportRequest(
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

ImmediateFuture<BackingStore::GetRootTreeResult>
HgQueuedBackingStore::getRootTree(
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

folly::Future<TreePtr> HgQueuedBackingStore::importTreeManifest(
    const ObjectId& commitId,
    const ObjectFetchContextPtr& context) {
  return folly::via(
             serverThreadPool_,
             [this, commitId] {
               return datapackStore_->getManifestNode(commitId);
             })
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

folly::Future<TreePtr> HgQueuedBackingStore::importTreeManifestImpl(
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
      datapackStore_->getTree(path.copy(), manifestNode, objectId, context);
  if (tree.hasValue()) {
    XLOG(DBG4) << "imported tree node=" << manifestNode << " path=" << path
               << " from SaplingNativeBackingStore";
    stats_->addDuration(&HgBackingStoreStats::fetchTree, watch.elapsed());
    return folly::makeFuture(std::move(tree.value()));
  }
  // retry once if the initial fetch failed
  return retryGetTree(manifestNode, objectId, path);
}

folly::Future<TreePtr> HgQueuedBackingStore::retryGetTree(
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
        stats_->addDuration(&HgBackingStoreStats::fetchTree, watch.elapsed());
        return std::move(result);
      });
}

folly::Future<TreePtr> HgQueuedBackingStore::retryGetTreeImpl(
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
               // SaplingNativeBackingStore (and HgDatapackStore) to provide and
               // asynchronous interface enabling us to perform our retries
               // there. In the meantime we use retryThreadPool_ for these
               // longer-running retry requests to avoid starving
               // serverThreadPool_.

               // Flush (and refresh) SaplingNativeBackingStore to ensure all
               // data is written and to rescan pack files or local indexes
               datapackStore_->flush();

               // Retry using datapackStore (SaplingNativeBackingStore)
               auto result = folly::makeFuture<TreePtr>(TreePtr{nullptr});
               auto tree = datapackStore_->getTree(
                   path, manifestNode, edenTreeID, /*context*/ nullptr);
               if (tree.hasValue()) {
                 stats_->increment(&HgBackingStoreStats::fetchTreeRetrySuccess);
                 result = tree.value();
               } else {
                 // Record miss and return error
                 if (structuredLogger_) {
                   structuredLogger_->logEvent(FetchMiss{
                       datapackStore_->getRepoName(),
                       FetchMiss::Tree,
                       tree.exception().what().toStdString(),
                       true});
                 }

                 stats_->increment(&HgBackingStoreStats::fetchTreeRetryFailure);
                 auto ew = folly::exception_wrapper{tree.exception()};
                 result = folly::makeFuture<TreePtr>(std::move(ew));
               }
               stats_->addDuration(
                   &HgBackingStoreStats::importTreeDuration, watch.elapsed());
               return result;
             })
      .thenError([this](folly::exception_wrapper&& ew) {
        stats_->increment(&HgBackingStoreStats::importTreeError);
        return folly::makeFuture<TreePtr>(std::move(ew));
      });
}

folly::SemiFuture<folly::Unit> HgQueuedBackingStore::prefetchBlobs(
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

void HgQueuedBackingStore::logMissingProxyHash() {
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

void HgQueuedBackingStore::logBackingStoreFetch(
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

size_t HgQueuedBackingStore::getImportMetric(
    RequestMetricsScope::RequestStage stage,
    HgImportObject object,
    RequestMetricsScope::RequestMetric metric) const {
  return RequestMetricsScope::getMetricFromWatches(
      metric, getImportWatches(stage, object));
}

RequestMetricsScope::LockedRequestWatchList&
HgQueuedBackingStore::getImportWatches(
    RequestMetricsScope::RequestStage stage,
    HgImportObject object) const {
  switch (stage) {
    case RequestMetricsScope::RequestStage::PENDING:
      return getPendingImportWatches(object);
    case RequestMetricsScope::RequestStage::LIVE:
      return getLiveImportWatches(object);
  }
  EDEN_BUG() << "unknown hg import stage " << enumValue(stage);
}

RequestMetricsScope::LockedRequestWatchList&
HgQueuedBackingStore::getPendingImportWatches(HgImportObject object) const {
  switch (object) {
    case HgImportObject::BLOB:
    case HgImportObject::BATCHED_BLOB:
      return pendingImportBlobWatches_;
    case HgImportObject::TREE:
    case HgImportObject::BATCHED_TREE:
      return pendingImportTreeWatches_;
    case HgImportObject::BLOBMETA:
    case HgImportObject::BATCHED_BLOBMETA:
      return pendingImportBlobMetaWatches_;
    case HgImportObject::PREFETCH:
      return pendingImportPrefetchWatches_;
  }
  EDEN_BUG() << "unknown hg import object type " << static_cast<int>(object);
}

RequestMetricsScope::LockedRequestWatchList&
HgQueuedBackingStore::getLiveImportWatches(HgImportObject object) const {
  switch (object) {
    case HgImportObject::BLOB:
      return liveImportBlobWatches_;
    case HgImportObject::TREE:
      return liveImportTreeWatches_;
    case HgImportObject::BLOBMETA:
      return liveImportBlobMetaWatches_;
    case HgImportObject::PREFETCH:
      return liveImportPrefetchWatches_;
    case HgImportObject::BATCHED_BLOB:
      return datapackStore_->getLiveBatchedBlobWatches();
    case HgImportObject::BATCHED_TREE:
      return datapackStore_->getLiveBatchedTreeWatches();
    case HgImportObject::BATCHED_BLOBMETA:
      return datapackStore_->getLiveBatchedBlobMetaWatches();
  }
  EDEN_BUG() << "unknown hg import object " << enumValue(object);
}

folly::StringPiece HgQueuedBackingStore::stringOfHgImportObject(
    HgImportObject object) {
  switch (object) {
    case HgImportObject::BLOB:
      return "blob";
    case HgImportObject::TREE:
      return "tree";
    case HgImportObject::BLOBMETA:
      return "blobmeta";
    case HgImportObject::BATCHED_BLOB:
      return "batched_blob";
    case HgImportObject::BATCHED_TREE:
      return "batched_tree";
    case HgImportObject::BATCHED_BLOBMETA:
      return "batched_blobmeta";
    case HgImportObject::PREFETCH:
      return "prefetch";
  }
  EDEN_BUG() << "unknown hg import object " << enumValue(object);
}

void HgQueuedBackingStore::startRecordingFetch() {
  isRecordingFetch_.store(true, std::memory_order_relaxed);
}

std::unordered_set<std::string> HgQueuedBackingStore::stopRecordingFetch() {
  isRecordingFetch_.store(false, std::memory_order_relaxed);
  std::unordered_set<std::string> paths;
  std::swap(paths, *fetchedFilePaths_.wlock());
  return paths;
}

ImmediateFuture<folly::Unit> HgQueuedBackingStore::importManifestForRoot(
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

void HgQueuedBackingStore::periodicManagementTask() {
  datapackStore_->flush();
}

namespace {
void dropBlobImportRequest(std::shared_ptr<HgImportRequest>& request) {
  auto* promise = request->getPromise<BlobPtr>();
  if (promise != nullptr) {
    if (!promise->isFulfilled()) {
      promise->setException(std::runtime_error("Request forcibly dropped"));
    }
  }
}

void dropTreeImportRequest(std::shared_ptr<HgImportRequest>& request) {
  auto* promise = request->getPromise<TreePtr>();
  if (promise != nullptr) {
    if (!promise->isFulfilled()) {
      promise->setException(std::runtime_error("Request forcibly dropped"));
    }
  }
}
} // namespace

int64_t HgQueuedBackingStore::dropAllPendingRequestsFromQueue() {
  auto requestVec = queue_.combineAndClearRequestQueues();
  for (auto& request : requestVec) {
    if (request->isType<HgImportRequest::BlobImport>()) {
      XLOG(DBG7, "Dropping blob request");
      dropBlobImportRequest(request);
    } else if (request->isType<HgImportRequest::TreeImport>()) {
      XLOG(DBG7, "Dropping tree request");
      dropTreeImportRequest(request);
    }
  }
  return requestVec.size();
}

} // namespace facebook::eden
