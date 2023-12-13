/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/hg/HgBackingStore.h"

#include <memory>

#include <folly/Range.h>
#include <folly/Synchronized.h>
#include <folly/ThreadLocal.h>
#include <folly/Try.h>
#include <folly/executors/CPUThreadPoolExecutor.h>
#include <folly/executors/GlobalExecutor.h>
#include <folly/executors/task_queue/UnboundedBlockingQueue.h>
#include <folly/executors/thread_factory/InitThreadFactory.h>
#include <folly/executors/thread_factory/NamedThreadFactory.h>
#include <folly/futures/Future.h>
#include <folly/logging/xlog.h>
#include <folly/stop_watch.h>

#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/eden-config.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/store/SerializedBlobMetadata.h"
#include "eden/fs/store/StoreResult.h"
#include "eden/fs/store/hg/HgDatapackStore.h"
#include "eden/fs/store/hg/HgImportPyError.h"
#include "eden/fs/store/hg/HgImportRequest.h"
#include "eden/fs/store/hg/HgImporter.h"
#include "eden/fs/store/hg/HgProxyHash.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/LogEvent.h"
#include "eden/fs/telemetry/RequestMetricsScope.h"
#include "eden/fs/telemetry/StructuredLogger.h"
#include "eden/fs/utils/Bug.h"
#include "eden/fs/utils/EnumValue.h"
#include "eden/fs/utils/Throw.h"
#include "eden/fs/utils/UnboundedQueueExecutor.h"

using folly::Future;
using folly::IOBuf;
using folly::makeFuture;
using folly::makeSemiFuture;
using folly::SemiFuture;
using folly::StringPiece;
using std::make_unique;

DEFINE_int32(
    num_hg_import_threads,
    // Why 8? 1 is materially slower but 24 is no better than 4 in a simple
    // microbenchmark that touches all files.  8 is better than 4 in the case
    // that we need to fetch a bunch from the network.
    // See benchmarks in the doc linked from D5067763.
    // Note that this number would benefit from occasional revisiting.
    8,
    "the number of hg import threads per repo");
DEFINE_bool(
    hg_fetch_missing_trees,
    true,
    "Set this parameter to \"no\" to disable fetching missing treemanifest "
    "trees from the remote mercurial server.  This is generally only useful "
    "for testing/debugging purposes");

namespace facebook::eden {

namespace {
// Thread local HgImporter. This is only initialized on HgImporter threads.
static folly::ThreadLocalPtr<Importer> threadLocalImporter;

/**
 * Checks that the thread local HgImporter is present and returns it.
 */
Importer& getThreadLocalImporter() {
  if (!threadLocalImporter) {
    EDEN_BUG() << "Attempting to get HgImporter from non-HgImporter thread";
  }
  return *threadLocalImporter;
}

ObjectId hashFromRootId(const RootId& root) {
  return ObjectId::fromHex(root.value());
}

/**
 * Thread factory that sets thread name and initializes a thread local
 * HgImporter.
 */
class HgImporterThreadFactory : public folly::InitThreadFactory {
 public:
  HgImporterThreadFactory(
      AbsolutePathPiece repository,
      EdenStatsPtr stats,
      std::shared_ptr<StructuredLogger> logger)
      : folly::InitThreadFactory(
            std::make_shared<folly::NamedThreadFactory>("HgImporter"),
            [repository = AbsolutePath{repository},
             stats = std::move(stats),
             logger] {
              threadLocalImporter.reset(
                  new HgImporterManager(repository, stats.copy(), logger));
            },
            [] {
              if (folly::kIsWindows) {
                // TODO(T125334969): On Windows, the ThreadLocalPtr doesn't
                // appear to release its resources when the thread dies, so
                // let's do it manually here.
                threadLocalImporter.reset();
              }
            }) {}
};

/**
 * An inline executor that, while it exists, keeps a thread-local HgImporter
 * instance.
 */
class HgImporterTestExecutor : public folly::InlineExecutor {
 public:
  explicit HgImporterTestExecutor(Importer* importer) : importer_{importer} {}

  void add(folly::Func f) override {
    // This is an InlineExecutor, so we may run on an arbitrary thread.
    threadLocalImporter.reset(importer_);
    SCOPE_EXIT {
      threadLocalImporter.release();
    };
    folly::InlineExecutor::add(std::move(f));
  }

 private:
  Importer* importer_;
};

HgDatapackStore::Options computeOptions() {
  HgDatapackStore::Options options{};
  options.allow_retries = false;
  return options;
}

HgDatapackStore::Options testOptions() {
  HgDatapackStore::Options options{};
  options.allow_retries = false;
  return options;
}

} // namespace

HgBackingStore::HgBackingStore(
    AbsolutePathPiece repository,
    std::shared_ptr<LocalStore> localStore,
    UnboundedQueueExecutor* serverThreadPool,
    std::shared_ptr<ReloadableConfig> config,
    EdenStatsPtr stats,
    std::shared_ptr<StructuredLogger> logger,
    FaultInjector* FOLLY_NONNULL faultInjector)
    : localStore_(std::move(localStore)),
      stats_(stats.copy()),
      importThreadPool_(make_unique<folly::CPUThreadPoolExecutor>(
          FLAGS_num_hg_import_threads,
          /* Eden performance will degrade when, for example, a status operation
           * causes a large number of import requests to be scheduled before a
           * lightweight operation needs to check the RocksDB cache. In that
           * case, the RocksDB threads can end up all busy inserting work into
           * the importer queue, preventing future requests that would hit cache
           * from succeeding.
           *
           * Thus, make the import queue unbounded.
           *
           * In the long term, we'll want a more comprehensive approach to
           * bounding the parallelism of scheduled work.
           */
          make_unique<folly::UnboundedBlockingQueue<
              folly::CPUThreadPoolExecutor::CPUTask>>(),
          std::make_shared<HgImporterThreadFactory>(
              repository,
              stats.copy(),
              logger))),
      config_(std::move(config)),
      serverThreadPool_(serverThreadPool),
      logger_(std::move(logger)),
      datapackStore_(
          repository,
          computeOptions(),
          config_,
          logger_,
          faultInjector) {}

/**
 * Create an HgBackingStore suitable for use in unit tests. It uses an inline
 * executor to process loaded objects rather than the thread pools used in
 * production Eden.
 */
HgBackingStore::HgBackingStore(
    AbsolutePathPiece repository,
    HgImporter* importer,
    std::shared_ptr<ReloadableConfig> config,
    std::shared_ptr<LocalStore> localStore,
    EdenStatsPtr stats,
    FaultInjector* FOLLY_NONNULL faultInjector)
    : localStore_{std::move(localStore)},
      stats_{std::move(stats)},
      importThreadPool_{std::make_unique<HgImporterTestExecutor>(importer)},
      config_(std::move(config)),
      serverThreadPool_{importThreadPool_.get()},
      logger_(nullptr),
      datapackStore_(
          repository,
          testOptions(),
          config_,
          logger_,
          faultInjector) {}

HgBackingStore::~HgBackingStore() = default;

ImmediateFuture<BackingStore::GetRootTreeResult> HgBackingStore::getRootTree(
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

SemiFuture<TreePtr> HgBackingStore::getTree(
    const std::shared_ptr<HgImportRequest>& request) {
  auto* treeImport = request->getRequest<HgImportRequest::TreeImport>();
  return importTreeImpl(
      treeImport->proxyHash.revHash(), // this is really the manifest node
      treeImport->hash,
      treeImport->proxyHash.path());
}

Future<TreePtr> HgBackingStore::importTreeImpl(
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
    return makeFuture(std::move(tree));
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
  return fetchTreeFromImporter(
             manifestNode, edenTreeID, path.copy(), std::move(writeBatch))
      .thenValue([this, watch, config = config_](TreePtr&& result) mutable {
        stats_->addDuration(&HgBackingStoreStats::fetchTree, watch.elapsed());
        return std::move(result);
      });
}

folly::Future<TreePtr> HgBackingStore::fetchTreeFromImporter(
    Hash20 manifestNode,
    ObjectId edenTreeID,
    RelativePath path,
    std::shared_ptr<LocalStore::WriteBatch> writeBatch) {
  return folly::via(
             importThreadPool_.get(),
             [this,
              path,
              manifestNode,
              edenTreeID,
              writeBatch,
              &liveImportTreeWatches = liveImportTreeWatches_] {
               Importer& importer = getThreadLocalImporter();
               folly::stop_watch<std::chrono::milliseconds> watch;
               RequestMetricsScope queueTracker{&liveImportTreeWatches};

               // NOTE: In the future we plan to update
               // SaplingNativeBackingStore (and HgDatapackStore) to provide and
               // asynchronous interface enabling us to perform our retries
               // there. In the meantime we use importThreadPool_ for these
               // longer-running retry requests to avoid starving
               // serverThreadPool_.

               // Flush (and refresh) SaplingNativeBackingStore to ensure all
               // data is written and to rescan pack files or local indexes
               datapackStore_.flush();

               // Retry using datapackStore (SaplingNativeBackingStore)
               auto result = folly::makeFuture<TreePtr>(TreePtr{nullptr});
               auto tree = datapackStore_.getTree(
                   path, manifestNode, edenTreeID, /*context*/ nullptr);
               if (tree.hasValue()) {
                 stats_->increment(&HgBackingStoreStats::fetchTreeRetrySuccess);
                 result = tree.value();
               } else if (config_->getEdenConfig()
                              ->hgImporterFetchFallback.getValue()) {
                 // Fall back to importer
                 auto serializedTree = importer.fetchTree(path, manifestNode);
                 if (serializedTree) {
                   stats_->increment(&HgBackingStoreStats::importTreeSuccess);
                 } else {
                   stats_->increment(&HgBackingStoreStats::importTreeFailure);
                 }
                 result = processTree(
                     std::move(serializedTree),
                     manifestNode,
                     edenTreeID,
                     path,
                     writeBatch.get());
               } else {
                 // No fallback to importer, record miss and return error
                 if (logger_) {
                   logger_->logEvent(FetchMiss{
                       datapackStore_.getRepoName(),
                       FetchMiss::BackingStore,
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

namespace {
constexpr size_t kNodeHexLen = Hash20::RAW_SIZE * 2;

struct ManifestEntry {
  Hash20 node;
  PathComponent name;
  TreeEntryType type;

  /**
   * Parse a manifest entry.
   *
   * The format of a Mercurial manifest is the following:
   * name: NUL terminated string
   * node: 40 bytes hex
   * flags: single character in: txl
   * <name><node><flag>\n
   */
  static ManifestEntry parse(const char** start, const char* end) {
    const auto* nameend =
        reinterpret_cast<const char*>(memchr(*start, '\0', end - *start));

    if (nameend == end) {
      throw std::domain_error("invalid manifest entry");
    }

    auto namePiece = StringPiece{*start, folly::to_unsigned(nameend - *start)};

    if (nameend + kNodeHexLen + 1 >= end) {
      throwf<std::domain_error>(
          FMT_STRING(
              "invalid manifest entry for {}: 40-bytes hash is too short: only {}-bytes available"),
          namePiece,
          nameend - end);
    }

    auto node = Hash20(StringPiece{nameend + 1, kNodeHexLen});

    auto flagsPtr = nameend + kNodeHexLen + 1;
    TreeEntryType type;
    switch (*flagsPtr) {
      case 't':
        type = TreeEntryType::TREE;
        *start = flagsPtr + 2;
        break;
      case 'x':
        type = TreeEntryType::EXECUTABLE_FILE;
        *start = flagsPtr + 2;
        break;
      case 'l':
        type = TreeEntryType::SYMLINK;
        *start = flagsPtr + 2;
        break;
      case '\n':
        type = TreeEntryType::REGULAR_FILE;
        *start = flagsPtr + 1;
        break;
      default:
        throw std::domain_error(fmt::format(
            FMT_STRING(
                "invalid manifest entry for {}: unsupported file flags: {}"),
            namePiece,
            *flagsPtr));
    }

    return ManifestEntry{node, PathComponent{namePiece}, type};
  }
};

class Manifest {
 public:
  explicit Manifest(std::unique_ptr<IOBuf> raw) {
    XDCHECK(!raw->isChained());

    auto start = reinterpret_cast<const char*>(raw->data());
    const auto end = reinterpret_cast<const char*>(raw->tail());

    while (start < end) {
      try {
        auto entry = ManifestEntry::parse(&start, end);
        entries_.push_back(std::move(entry));
      } catch (const PathComponentContainsDirectorySeparator& ex) {
        XLOG(WARN) << "Ignoring directory entry: " << ex.what();
      }
    }
  }

  Manifest(const Manifest&) = delete;
  Manifest(Manifest&&) = delete;
  Manifest& operator=(const Manifest&) = delete;
  Manifest& operator=(Manifest&&) = delete;

  ~Manifest() = default;

  using iterator = std::vector<ManifestEntry>::iterator;

  iterator begin() {
    return entries_.begin();
  }

  iterator end() {
    return entries_.end();
  }

 private:
  std::vector<ManifestEntry> entries_;
};

} // namespace

TreePtr HgBackingStore::processTree(
    std::unique_ptr<IOBuf> content,
    const Hash20& manifestNode,
    const ObjectId& edenTreeID,
    RelativePathPiece path,
    LocalStore::WriteBatch* writeBatch) {
  auto manifest = Manifest(std::move(content));
  Tree::container entries{kPathMapDefaultCaseSensitive};
  auto hgObjectIdFormat = config_->getEdenConfig()->hgObjectIdFormat.getValue();
  const auto filteredPaths =
      config_->getEdenConfig()->hgFilteredPaths.getValue();

  for (auto& entry : manifest) {
    XLOG(DBG9) << "tree: " << manifestNode << " " << entry.name
               << " node: " << entry.node << " flag: " << entry.type;

    auto relPath = path + entry.name;
    if (filteredPaths->count(relPath) == 0) {
      auto proxyHash =
          HgProxyHash::store(relPath, entry.node, hgObjectIdFormat);

      entries.emplace(entry.name, proxyHash, entry.type);
    }
  }

  writeBatch->flush();

  return std::make_shared<TreePtr::element_type>(
      std::move(entries), edenTreeID);
}

ImmediateFuture<folly::Unit> HgBackingStore::importTreeManifestForRoot(
    const RootId& rootId,
    const Hash20& manifestId,
    const ObjectFetchContextPtr& context) {
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

folly::Future<TreePtr> HgBackingStore::importTreeManifest(
    const ObjectId& commitId,
    const ObjectFetchContextPtr& context) {
  return folly::via(
             serverThreadPool_,
             [this, commitId] {
               return datapackStore_.getManifestNode(commitId);
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

folly::Future<TreePtr> HgBackingStore::importTreeManifestImpl(
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

  // try SaplingNativeBackingStore first
  folly::stop_watch<std::chrono::milliseconds> watch;
  auto tree =
      datapackStore_.getTree(path.copy(), manifestNode, objectId, context);
  if (tree.hasValue()) {
    XLOG(DBG4) << "imported tree node=" << manifestNode << " path=" << path
               << " from SaplingNativeBackingStore";
    stats_->addDuration(&HgBackingStoreStats::fetchTree, watch.elapsed());
    return folly::makeFuture(std::move(tree.value()));
  }

  return importTreeImpl(manifestNode, objectId, path);
}

SemiFuture<BlobPtr> HgBackingStore::fetchBlobFromHgImporter(
    HgProxyHash hgInfo) {
  return folly::via(
             importThreadPool_.get(),
             [this,
              hgInfo = std::move(hgInfo),
              &liveImportBlobWatches = liveImportBlobWatches_] {
               folly::stop_watch<std::chrono::milliseconds> watch;
               RequestMetricsScope queueTracker{&liveImportBlobWatches};

               // NOTE: In the future we plan to update
               // SaplingNativeBackingStore (and HgDatapackStore) to provide and
               // asynchronous interface enabling us to perform our retries
               // there. In the meantime we use importThreadPool_ for these
               // longer-running retry requests to avoid starving
               // serverThreadPool_.

               // Flush (and refresh) SaplingNativeBackingStore to ensure all
               // data is written and to rescan pack files or local indexes
               datapackStore_.flush();

               // Retry using datapackStore (SaplingNativeBackingStore).
               auto result = folly::makeFuture<BlobPtr>(BlobPtr{nullptr});
               auto blob = datapackStore_.getBlob(hgInfo, /*localOnly=*/false);
               if (blob.hasValue()) {
                 stats_->increment(&HgBackingStoreStats::fetchBlobRetrySuccess);
                 result = blob.value();
               } else if (config_->getEdenConfig()
                              ->hgImporterFetchFallback.getValue()) {
                 // Fall back to importer
                 Importer& importer = getThreadLocalImporter();
                 result = importer.importFileContents(
                     hgInfo.path(), hgInfo.revHash());

                 if (result.hasValue()) {
                   stats_->increment(&HgBackingStoreStats::importBlobSuccess);
                 } else {
                   stats_->increment(&HgBackingStoreStats::importBlobFailure);
                 }
               } else {
                 // No fallback to importer, record miss and return error
                 if (logger_) {
                   logger_->logEvent(FetchMiss{
                       datapackStore_.getRepoName(),
                       FetchMiss::BackingStore,
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

folly::StringPiece HgBackingStore::stringOfHgImportObject(
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

RequestMetricsScope::LockedRequestWatchList&
HgBackingStore::getLiveImportWatches(HgImportObject object) const {
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
      return datapackStore_.getLiveBatchedBlobWatches();
    case HgImportObject::BATCHED_TREE:
      return datapackStore_.getLiveBatchedTreeWatches();
    case HgImportObject::BATCHED_BLOBMETA:
      return datapackStore_.getLiveBatchedBlobMetaWatches();
  }
  EDEN_BUG() << "unknown hg import object " << enumValue(object);
}

void HgBackingStore::periodicManagementTask() {
  datapackStore_.flush();
}

} // namespace facebook::eden
