/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "HgBackingStore.h"

#include <memory>

#include <folly/Range.h>
#include <folly/Synchronized.h>
#include <folly/ThreadLocal.h>
#include <folly/Try.h>
#include <folly/executors/CPUThreadPoolExecutor.h>
#include <folly/executors/GlobalExecutor.h>
#include <folly/executors/task_queue/UnboundedBlockingQueue.h>
#include <folly/executors/thread_factory/NamedThreadFactory.h>
#include <folly/futures/Future.h>
#include <folly/logging/xlog.h>
#include <folly/stop_watch.h>

#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/eden-config.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/service/ThriftUtil.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/store/SerializedBlobMetadata.h"
#include "eden/fs/store/StoreResult.h"
#include "eden/fs/store/TreeMetadata.h"
#include "eden/fs/store/hg/HgDatapackStore.h"
#include "eden/fs/store/hg/HgImportPyError.h"
#include "eden/fs/store/hg/HgImportRequest.h"
#include "eden/fs/store/hg/HgImporter.h"
#include "eden/fs/store/hg/HgProxyHash.h"
#include "eden/fs/store/hg/MetadataImporter.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/RequestMetricsScope.h"
#include "eden/fs/utils/Bug.h"
#include "eden/fs/utils/EnumValue.h"
#include "eden/fs/utils/UnboundedQueueExecutor.h"

#ifdef EDEN_HAVE_SERVICEROUTER
#include "servicerouter/common/TServiceRouterException.h" // @manual
#include "servicerouter/common/gen-cpp2/error_types.h" // @manual

using facebook::servicerouter::ErrorReason;
using facebook::servicerouter::TServiceRouterException;
#endif

using folly::Future;
using folly::IOBuf;
using folly::makeFuture;
using folly::SemiFuture;
using folly::StringPiece;
using std::make_unique;
using std::unique_ptr;

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
    throw std::logic_error(
        "Attempting to get HgImporter from non-HgImporter thread");
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
class HgImporterThreadFactory : public folly::ThreadFactory {
 public:
  HgImporterThreadFactory(
      AbsolutePathPiece repository,
      std::shared_ptr<EdenStats> stats)
      : delegate_("HgImporter"),
        repository_(repository),
        stats_(std::move(stats)) {}

  std::thread newThread(folly::Func&& func) override {
    return delegate_.newThread([this, func = std::move(func)]() mutable {
      threadLocalImporter.reset(new HgImporterManager(repository_, stats_));
      SCOPE_EXIT {
        // TODO(xavierd): On Windows, the ThreadLocalPtr doesn't appear to
        // release its resources when the thread dies, so let's do it manually
        // here.
        threadLocalImporter.reset();
      };
      func();
    });
  }

 private:
  folly::NamedThreadFactory delegate_;
  AbsolutePath repository_;
  std::shared_ptr<EdenStats> stats_;
};

/**
 * An inline executor that, while it exists, keeps a thread-local HgImporter
 * instance.
 */
class HgImporterTestExecutor : public folly::InlineExecutor {
 public:
  explicit HgImporterTestExecutor(Importer* importer) {
    threadLocalImporter.reset(importer);
  }

  ~HgImporterTestExecutor() override {
    threadLocalImporter.release();
  }
};

} // namespace

HgBackingStore::HgBackingStore(
    AbsolutePathPiece repository,
    std::shared_ptr<LocalStore> localStore,
    UnboundedQueueExecutor* serverThreadPool,
    std::shared_ptr<ReloadableConfig> config,
    std::shared_ptr<EdenStats> stats,
    MetadataImporterFactory metadataImporterFactory)
    : localStore_(std::move(localStore)),
      stats_(stats),
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
          std::make_shared<HgImporterThreadFactory>(repository, stats))),
      config_(config),
      serverThreadPool_(serverThreadPool),
      datapackStore_(
          repository,
          config->getEdenConfig()->useEdenApi.getValue(),
          config->getEdenConfig()->useAuxMetadata.getValue(),
          config) {
  HgImporter importer(repository, stats);
  const auto& options = importer.getOptions();
  repoName_ = options.repoName;
  metadataImporter_ = metadataImporterFactory(config_, repoName_, localStore_);
}

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
    std::shared_ptr<EdenStats> stats)
    : HgBackingStore(
          repository,
          std::move(importer),
          std::move(config),
          std::move(localStore),
          stats,
          MetadataImporter::getMetadataImporterFactory<
              DefaultMetadataImporter>()) {}

HgBackingStore::HgBackingStore(
    AbsolutePathPiece repository,
    HgImporter* importer,
    std::shared_ptr<ReloadableConfig> config,
    std::shared_ptr<LocalStore> localStore,
    std::shared_ptr<EdenStats> stats,
    MetadataImporterFactory metadataImporterFactory)
    : localStore_{std::move(localStore)},
      stats_{std::move(stats)},
      importThreadPool_{std::make_unique<HgImporterTestExecutor>(importer)},
      config_(std::move(config)),
      serverThreadPool_{importThreadPool_.get()},
      datapackStore_(repository, false, false, config_) {
  const auto& options = importer->getOptions();
  repoName_ = options.repoName;
  metadataImporter_ = metadataImporterFactory(config_, repoName_, localStore_);
}

HgBackingStore::~HgBackingStore() = default;

SemiFuture<unique_ptr<Tree>> HgBackingStore::getRootTree(
    const RootId& rootId,
    bool prefetchMetadata) {
  ObjectId commitId = hashFromRootId(rootId);

  return localStore_
      ->getFuture(KeySpace::HgCommitToTreeFamily, commitId.getBytes())
      .thenValue(
          [this, commitId, prefetchMetadata](
              StoreResult result) -> folly::SemiFuture<unique_ptr<Tree>> {
            if (!result.isValid()) {
              return importTreeManifest(commitId, prefetchMetadata)
                  .thenValue([this, commitId](std::unique_ptr<Tree> rootTree) {
                    XLOG(DBG1) << "imported mercurial commit " << commitId
                               << " as tree " << rootTree->getHash();

                    localStore_->put(
                        KeySpace::HgCommitToTreeFamily,
                        commitId,
                        rootTree->getHash().getBytes());
                    return rootTree;
                  });
            }

            auto rootTreeHash = HgProxyHash::load(
                localStore_.get(), ObjectId{result.bytes()}, "getRootTree");
            return importTreeManifestImpl(
                rootTreeHash.revHash(), prefetchMetadata);
          });
}

SemiFuture<unique_ptr<Tree>> HgBackingStore::getTree(
    const std::shared_ptr<HgImportRequest>& request) {
  auto* treeImport = request->getRequest<HgImportRequest::TreeImport>();
  return importTreeImpl(
      treeImport->proxyHash.revHash(), // this is really the manifest node
      treeImport->hash,
      treeImport->proxyHash.path(),
      treeImport->prefetchMetadata);
}

void HgBackingStore::getTreeBatch(
    const std::vector<std::shared_ptr<HgImportRequest>>& requests,
    bool prefetchMetadata) {
  std::vector<folly::Promise<std::unique_ptr<Tree>>> innerPromises;
  innerPromises.reserve(requests.size());
  std::vector<folly::SemiFuture<std::unique_ptr<TreeMetadata>>> metadataFutures;
  metadataFutures.reserve(requests.size());

  // When aux metadata is enabled hg fetches file metadata along with get tree
  // request, no need for separate network call!
  bool useAuxMetadata = config_->getEdenConfig()->useAuxMetadata.getValue();
  bool metadataEnabled = metadataImporter_->metadataFetchingAvailable() &&
      prefetchMetadata && !useAuxMetadata;

  // Kick off all the fetching
  for (const auto& request : requests) {
    innerPromises.emplace_back(folly::Promise<std::unique_ptr<Tree>>());

    auto treeMetadataFuture =
        folly::SemiFuture<std::unique_ptr<TreeMetadata>>::makeEmpty();
    if (metadataEnabled) {
      auto* treeImport = request->getRequest<HgImportRequest::TreeImport>();
      treeMetadataFuture = metadataImporter_->getTreeMetadata(
          treeImport->hash, treeImport->proxyHash.revHash());
    }
    metadataFutures.push_back(std::move(treeMetadataFuture));
  }

  {
    auto writeBatch = localStore_->beginWrite();
    datapackStore_.getTreeBatch(requests, writeBatch.get(), &innerPromises);
  }

  // Receive the fetches and tie the content and metadata together if needed.
  auto requestIt = requests.begin();
  auto treeMetadataFuture = std::make_move_iterator(metadataFutures.begin());
  for (auto innerPromise = innerPromises.begin();
       innerPromise != innerPromises.end();
       ++innerPromise, ++treeMetadataFuture, ++requestIt) {
    // This innerPromise pattern is so we can retrieve the tree from the
    // innerPromise and use it for tree metadata prefetching, without
    // invalidating the passed in Promise.
    if (innerPromise->isFulfilled()) {
      (*requestIt)->getPromise<std::unique_ptr<Tree>>()->setWith([&]() mutable {
        std::unique_ptr<Tree> tree = innerPromise->getSemiFuture().get();
        this->processTreeMetadata(std::move(*treeMetadataFuture), *tree);
        return tree;
      });
    }
  }
}

Future<unique_ptr<Tree>> HgBackingStore::importTreeImpl(
    const Hash20& manifestNode,
    const ObjectId& edenTreeID,
    RelativePathPiece path,
    bool prefetchMetadata) {
  XLOG(DBG6) << "importing tree " << edenTreeID << ": hg manifest "
             << manifestNode << " for path \"" << path << "\"";

  // Explicitly check for the null ID on the root directory.
  // This isn't actually present in the mercurial data store; it has to be
  // handled specially in the code.
  if (path.empty() && manifestNode == kZeroHash) {
    auto tree = make_unique<Tree>(std::vector<TreeEntry>{}, edenTreeID);
    return makeFuture(std::move(tree));
  }

  folly::stop_watch<std::chrono::milliseconds> watch;

  auto treeMetadataFuture =
      folly::SemiFuture<std::unique_ptr<TreeMetadata>>::makeEmpty();
  // When aux metadata is enabled hg fetches file metadata along with get tree
  // request, no need for separate network call!
  bool useAuxMetadata = config_->getEdenConfig()->useAuxMetadata.getValue();
  if (metadataImporter_->metadataFetchingAvailable() && prefetchMetadata &&
      !useAuxMetadata) {
    treeMetadataFuture =
        metadataImporter_->getTreeMetadata(edenTreeID, manifestNode);
  }
  return fetchTreeFromHgCacheOrImporter(manifestNode, edenTreeID, path.copy())
      .thenValue([this,
                  watch,
                  treeMetadataFuture = std::move(treeMetadataFuture),
                  config = config_](std::unique_ptr<Tree>&& result) mutable {
        auto& currentThreadStats =
            stats_->getHgBackingStoreStatsForCurrentThread();
        currentThreadStats.hgBackingStoreGetTree.addValue(
            watch.elapsed().count());
        this->processTreeMetadata(std::move(treeMetadataFuture), *result);
        return std::move(result);
      });
}

void HgBackingStore::processTreeMetadata(
    folly::SemiFuture<std::unique_ptr<TreeMetadata>>&& treeMetadataFuture,
    const Tree& tree) {
  if (!treeMetadataFuture.valid()) {
    return;
  }

  // metadata fetching will need the eden ids of each of the
  // children of the the tree, to store the metadata for each of the
  // children in the local store. Thus we make a copy of this and
  // pass it along to metadata storage.
  std::move(treeMetadataFuture)
      .via(serverThreadPool_)
      .thenValue([localStore = localStore_, tree = tree](
                     std::unique_ptr<TreeMetadata>&& treeMetadata) mutable {
        // note this may throw if the localStore has already been
        // closed
        localStore->putTreeMetadata(*treeMetadata, tree);
      })
      .thenError([config = config_](folly::exception_wrapper&& error) {
#ifdef EDEN_HAVE_SERVICEROUTER
        if (TServiceRouterException* serviceRouterError =
                error.get_exception<TServiceRouterException>()) {
          if (config &&
              serviceRouterError->getErrorReason() ==
                  ErrorReason::THROTTLING_REQUEST) {
            XLOG_EVERY_N_THREAD(
                WARN,
                config->getEdenConfig()->scsThrottleErrorSampleRatio.getValue())
                << "Error during metadata pre-fetching or storage: "
                << error.what();
            return;
          }
        }
#endif
        XLOG(WARN) << "Error during metadata pre-fetching or storage: "
                   << error.what();
      });
}

folly::Future<std::unique_ptr<Tree>>
HgBackingStore::fetchTreeFromHgCacheOrImporter(
    Hash20 manifestNode,
    ObjectId edenTreeID,
    RelativePath path) {
  auto writeBatch = localStore_->beginWrite();
  if (auto tree = datapackStore_.getTree(
          path, manifestNode, edenTreeID, writeBatch.get())) {
    XLOG(DBG4) << "imported tree node=" << manifestNode << " path=" << path
               << " from Rust hgcache";
    return folly::makeFuture(std::move(tree));
  } else {
    // Data for this tree was not present locally.
    // Fall through and fetch the data from the server below.
    if (!FLAGS_hg_fetch_missing_trees) {
      auto ew = folly::exception_wrapper(std::current_exception());
      return folly::makeFuture<unique_ptr<Tree>>(ew);
    }
    return fetchTreeFromImporter(
        manifestNode, edenTreeID, std::move(path), std::move(writeBatch));
  }
}

folly::Future<std::unique_ptr<Tree>> HgBackingStore::fetchTreeFromImporter(
    Hash20 manifestNode,
    ObjectId edenTreeID,
    RelativePath path,
    std::shared_ptr<LocalStore::WriteBatch> writeBatch) {
  auto fut =
      folly::via(
          importThreadPool_.get(),
          [path,
           manifestNode,
           stats = stats_,
           &liveImportTreeWatches = liveImportTreeWatches_] {
            Importer& importer = getThreadLocalImporter();
            folly::stop_watch<std::chrono::milliseconds> watch;
            RequestMetricsScope queueTracker{&liveImportTreeWatches};

            auto serializedTree = importer.fetchTree(path, manifestNode);
            stats->getHgBackingStoreStatsForCurrentThread()
                .hgBackingStoreImportTree.addValue(watch.elapsed().count());

            return serializedTree;
          })
          .via(serverThreadPool_);

  return std::move(fut).thenTry([this,
                                 ownedPath = std::move(path),
                                 node = std::move(manifestNode),
                                 treeID = std::move(edenTreeID),
                                 batch = std::move(writeBatch)](
                                    folly::Try<std::unique_ptr<IOBuf>> val) {
    // Note: the `value` call will throw if fetchTree threw an exception
    auto iobuf = std::move(val).value();
    return processTree(std::move(iobuf), node, treeID, ownedPath, batch.get());
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
      throw std::domain_error(fmt::format(
          FMT_STRING(
              "invalid manifest entry for {}: 40-bytes hash is too short: only {}-bytes available"),
          namePiece,
          nameend - end));
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

std::unique_ptr<Tree> HgBackingStore::processTree(
    std::unique_ptr<IOBuf> content,
    const Hash20& manifestNode,
    const ObjectId& edenTreeID,
    RelativePathPiece path,
    LocalStore::WriteBatch* writeBatch) {
  auto manifest = Manifest(std::move(content));
  std::vector<TreeEntry> entries;
  auto directObjectId = config_->getEdenConfig()->directObjectId.getValue();

  for (auto& entry : manifest) {
    XLOG(DBG9) << "tree: " << manifestNode << " " << entry.name
               << " node: " << entry.node << " flag: " << entry.type;

    auto relPath = path + entry.name;
    auto proxyHash = HgProxyHash::store(
        relPath, entry.node, directObjectId ? nullptr : writeBatch);

    entries.emplace_back(proxyHash, std::move(entry.name), entry.type);
  }

  writeBatch->flush();

  return make_unique<Tree>(std::move(entries), edenTreeID);
}

folly::Future<folly::Unit> HgBackingStore::importTreeManifestForRoot(
    const RootId& rootId,
    const Hash20& manifestId,
    bool prefetchMetadata) {
  auto commitId = hashFromRootId(rootId);
  return localStore_
      ->getFuture(KeySpace::HgCommitToTreeFamily, commitId.getBytes())
      .thenValue(
          [this, commitId, manifestId, prefetchMetadata](
              StoreResult result) -> folly::Future<folly::Unit> {
            if (result.isValid()) {
              // We have already imported this commit, nothing to do.
              return folly::unit;
            }

            return importTreeManifestImpl(manifestId, prefetchMetadata)
                .thenValue([this, commitId, manifestId](
                               std::unique_ptr<Tree> rootTree) {
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

folly::Future<std::unique_ptr<Tree>> HgBackingStore::importTreeManifest(
    const ObjectId& commitId,
    bool prefetchMetadata) {
  return folly::via(
             importThreadPool_.get(),
             [commitId] {
               return getThreadLocalImporter().resolveManifestNode(
                   commitId.asHexString());
             })
      .via(serverThreadPool_)
      .thenValue([this, commitId, prefetchMetadata](auto manifestNode) {
        XLOG(DBG2) << "revision " << commitId << " has manifest node "
                   << manifestNode;
        return importTreeManifestImpl(manifestNode, prefetchMetadata);
      });
}

folly::Future<std::unique_ptr<Tree>> HgBackingStore::importTreeManifestImpl(
    Hash20 manifestNode,
    bool prefetchMetadata) {
  // Record that we are at the root for this node
  RelativePathPiece path{};
  auto directObjectId = config_->getEdenConfig()->directObjectId.getValue();
  ObjectId objectId;
  std::pair<ObjectId, std::string> computedPair;
  if (directObjectId) { // unfortunately we have to know about internals of
                        // proxy hash here
    objectId = HgProxyHash::makeEmbeddedProxyHash(manifestNode);
  } else {
    computedPair = HgProxyHash::prepareToStoreLegacy(path, manifestNode);
    objectId = computedPair.first;
  }
  auto futTree = importTreeImpl(manifestNode, objectId, path, prefetchMetadata);
  if (directObjectId) {
    return futTree;
  } else {
    return std::move(futTree).thenValue(
        [computedPair, batch = localStore_->beginWrite()](auto tree) {
          // Only write the proxy hash value for this once we've imported
          // the root.
          HgProxyHash::storeLegacy(computedPair, batch.get());
          batch->flush();
          return tree;
        });
  }
}

unique_ptr<Tree> HgBackingStore::getTreeFromHgCache(
    const ObjectId& edenTreeId,
    const HgProxyHash& proxyHash,
    bool prefetchMetadata) {
  if (auto tree =
          datapackStore_.getTreeLocal(edenTreeId, proxyHash, *localStore_)) {
    XLOG(DBG5) << "imported tree of '" << proxyHash.path() << "', "
               << proxyHash.revHash().toString() << " from hgcache";

    auto treeMetadataFuture =
        folly::SemiFuture<std::unique_ptr<TreeMetadata>>::makeEmpty();
    bool useAuxMetadata = config_->getEdenConfig()->useAuxMetadata.getValue();
    if (metadataImporter_->metadataFetchingAvailable() && prefetchMetadata &&
        !useAuxMetadata) {
      treeMetadataFuture =
          metadataImporter_->getTreeMetadata(edenTreeId, proxyHash.revHash());
    }
    this->processTreeMetadata(std::move(treeMetadataFuture), *tree);
    return tree;
  }

  return nullptr;
}

SemiFuture<std::unique_ptr<Blob>> HgBackingStore::fetchBlobFromHgImporter(
    HgProxyHash hgInfo) {
  return folly::via(
      importThreadPool_.get(),
      [stats = stats_,
       hgInfo = std::move(hgInfo),
       &liveImportBlobWatches = liveImportBlobWatches_] {
        Importer& importer = getThreadLocalImporter();
        folly::stop_watch<std::chrono::milliseconds> watch;
        RequestMetricsScope queueTracker{&liveImportBlobWatches};
        auto blob =
            importer.importFileContents(hgInfo.path(), hgInfo.revHash());
        stats->getHgBackingStoreStatsForCurrentThread()
            .hgBackingStoreImportBlob.addValue(watch.elapsed().count());
        return blob;
      });
}

SemiFuture<folly::Unit> HgBackingStore::prefetchBlobs(
    std::vector<HgProxyHash> proxyHashes,
    ObjectFetchContext& /*context*/) {
  return folly::via(
             importThreadPool_.get(),
             [proxyHashes = std::move(proxyHashes),
              &liveImportPrefetchWatches = liveImportPrefetchWatches_] {
               RequestMetricsScope queueTracker{&liveImportPrefetchWatches};
               return getThreadLocalImporter().prefetchFiles(proxyHashes);
             })
      .via(serverThreadPool_);
}

folly::StringPiece HgBackingStore::stringOfHgImportObject(
    HgImportObject object) {
  switch (object) {
    case HgImportObject::BLOB:
      return "blob";
    case HgImportObject::TREE:
      return "tree";
    case HgImportObject::BATCHED_BLOB:
      return "batched_blob";
    case HgImportObject::BATCHED_TREE:
      return "batched_tree";
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
    case HgImportObject::PREFETCH:
      return liveImportPrefetchWatches_;
    case HgImportObject::BATCHED_BLOB:
      return datapackStore_.getLiveBatchedBlobWatches();
    case HgImportObject::BATCHED_TREE:
      return datapackStore_.getLiveBatchedTreeWatches();
  }
  EDEN_BUG() << "unknown hg import object " << enumValue(object);
}

void HgBackingStore::periodicManagementTask() {
  datapackStore_.flush();
}

} // namespace facebook::eden
