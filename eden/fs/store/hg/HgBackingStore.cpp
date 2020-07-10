/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "HgBackingStore.h"

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
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/store/SerializedBlobMetadata.h"
#include "eden/fs/store/StoreResult.h"
#include "eden/fs/store/hg/HgDatapackStore.h"
#include "eden/fs/store/hg/HgImportPyError.h"
#include "eden/fs/store/hg/HgImporter.h"
#include "eden/fs/store/hg/HgProxyHash.h"
#include "eden/fs/store/hg/ScsProxyHash.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/RequestMetricsScope.h"
#include "eden/fs/utils/Bug.h"
#include "eden/fs/utils/EnumValue.h"
#include "eden/fs/utils/UnboundedQueueExecutor.h"

#include "edenscm/hgext/extlib/cstore/uniondatapackstore.h" // @manual=//eden/scm:datapack
#include "edenscm/hgext/extlib/ctreemanifest/treemanifest.h" // @manual=//eden/scm:datapack

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

namespace facebook {
namespace eden {

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

// A helper function to avoid repeating noisy casts/conversions when
// loading data from a UnionDatapackStore instance.
ConstantStringRef unionStoreGet(
    UnionDatapackStore& unionStore,
    StringPiece name,
    const Hash& id) {
  return unionStore.get(
      Key(name.data(),
          name.size(),
          (const char*)id.getBytes().data(),
          id.getBytes().size()));
}

// A helper function to avoid repeating noisy casts/conversions when
// loading data from a UnionDatapackStore instance.  This variant will
// ask the store to rescan and look for changed packs if it encounters
// a missing key.
ConstantStringRef unionStoreGetWithRefresh(
    UnionDatapackStore& unionStore,
    StringPiece name,
    const Hash& id) {
  try {
    return unionStoreGet(unionStore, name, id);
  } catch (const MissingKeyError&) {
    unionStore.markForRefresh();
    return unionStoreGet(unionStore, name, id);
  }
}
} // namespace

HgBackingStore::HgBackingStore(
    AbsolutePathPiece repository,
    std::shared_ptr<LocalStore> localStore,
    UnboundedQueueExecutor* serverThreadPool,
    std::shared_ptr<ReloadableConfig> config,
    std::shared_ptr<EdenStats> stats)
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
          config->getEdenConfig()->useEdenApi.getValue()) {
  HgImporter importer(repository, stats);
  const auto& options = importer.getOptions();
  initializeTreeManifestImport(options, repository);
  repoName_ = options.repoName;
}

/**
 * Create an HgBackingStore suitable for use in unit tests. It uses an inline
 * executor to process loaded objects rather than the thread pools used in
 * production Eden.
 */
HgBackingStore::HgBackingStore(
    AbsolutePathPiece repository,
    HgImporter* importer,
    std::shared_ptr<LocalStore> localStore,
    std::shared_ptr<EdenStats> stats)
    : localStore_{std::move(localStore)},
      stats_{std::move(stats)},
      importThreadPool_{std::make_unique<HgImporterTestExecutor>(importer)},
      serverThreadPool_{importThreadPool_.get()},
      datapackStore_(repository, false) {
  const auto& options = importer->getOptions();
  initializeTreeManifestImport(options, repository);
  repoName_ = options.repoName;
}

HgBackingStore::~HgBackingStore() {}

void HgBackingStore::initializeTreeManifestImport(
    const ImporterOptions& options,
    AbsolutePathPiece repoPath) {
  if (options.treeManifestPackPaths.empty()) {
    throw std::runtime_error(folly::to<std::string>(
        "treemanifest import not supported in repository ", repoPath));
  }

  std::vector<DataStore*> storePtrs;
  for (const auto& path : options.treeManifestPackPaths) {
    XLOG(DBG5) << "treemanifest pack path: " << path;
    // Create a new DatapackStore for path.  Note that we enable removing
    // dead pack files.  This is only guaranteed to be safe so long as we copy
    // the relevant data out of the datapack objects before we issue a
    // subsequent call into the unionStore_.
    dataPackStores_.emplace_back(std::make_unique<DatapackStore>(path, true));
    storePtrs.emplace_back(dataPackStores_.back().get());
  }

  unionStore_ = std::make_unique<folly::Synchronized<UnionDatapackStore>>(
      folly::in_place, storePtrs);
  XLOG(DBG2) << "treemanifest import enabled in repository " << repoPath;
}

SemiFuture<unique_ptr<Tree>> HgBackingStore::getTree(
    const Hash& id,
    ObjectFetchContext& /*context*/,
    ImportPriority /* priority */) {
  HgProxyHash pathInfo(localStore_.get(), id, "importTree");
  std::optional<Hash> commitHash;
  // note: if the parent of the tree was fetched with an old version of eden
  // then the commit id will not be available
  if (auto commitInfo =
          ScsProxyHash::load(localStore_.get(), id, "importTree")) {
    commitHash = commitInfo.value().commitHash();
  }
  return importTreeImpl(
      pathInfo.revHash(), // this is really the manifest node
      id,
      pathInfo.path(),
      commitHash);
}

Future<unique_ptr<Tree>> HgBackingStore::importTreeImpl(
    const Hash& manifestNode,
    const Hash& edenTreeID,
    RelativePathPiece path,
    const std::optional<Hash>& commitHash) {
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

  return fetchTreeFromHgCacheOrImporter(
             manifestNode, edenTreeID, path.copy(), commitHash)
      .thenValue([stats = stats_, watch](auto&& result) {
        auto& currentThreadStats =
            stats->getHgBackingStoreStatsForCurrentThread();
        currentThreadStats.hgBackingStoreGetTree.addValue(
            watch.elapsed().count());
        return std::move(result);
      });
}

folly::Future<std::unique_ptr<Tree>>
HgBackingStore::fetchTreeFromHgCacheOrImporter(
    Hash manifestNode,
    Hash edenTreeID,
    RelativePath path,
    const std::optional<Hash>& commitId) {
  auto writeBatch = localStore_->beginWrite();
  try {
    if (auto tree = datapackStore_.getTree(
            path, manifestNode, edenTreeID, writeBatch.get(), commitId)) {
      XLOG(DBG4) << "imported tree node=" << manifestNode << " path=" << path
                 << " from Rust hgcache";
      return folly::makeFuture(std::move(tree));
    }
    auto content = unionStoreGetWithRefresh(
        *unionStore_->wlock(), path.stringPiece(), manifestNode);
    return folly::makeFuture(processTree(
        content, manifestNode, edenTreeID, path, commitId, writeBatch.get()));
  } catch (const MissingKeyError&) {
    // Data for this tree was not present locally.
    // Fall through and fetch the data from the server below.
    if (!FLAGS_hg_fetch_missing_trees) {
      auto ew = folly::exception_wrapper(std::current_exception());
      return folly::makeFuture<unique_ptr<Tree>>(ew);
    }
    return fetchTreeFromImporter(
        manifestNode,
        edenTreeID,
        std::move(path),
        commitId,
        std::move(writeBatch));
  }
}

folly::Future<std::unique_ptr<Tree>> HgBackingStore::fetchTreeFromImporter(
    Hash manifestNode,
    Hash edenTreeID,
    RelativePath path,
    std::optional<Hash> commitId,
    std::shared_ptr<LocalStore::WriteBatch> writeBatch) {
  return folly::via(
             importThreadPool_.get(),
             [path,
              manifestNode,
              stats = stats_,
              &liveImportTreeWatches = liveImportTreeWatches_] {
               Importer& importer = getThreadLocalImporter();
               folly::stop_watch<std::chrono::milliseconds> watch;
               RequestMetricsScope queueTracker{&liveImportTreeWatches};
               importer.fetchTree(path, manifestNode);
               stats->getHgBackingStoreStatsForCurrentThread()
                   .hgBackingStoreImportTree.addValue(watch.elapsed().count());
             })
      .via(serverThreadPool_)
      .thenTry([this,
                ownedPath = std::move(path),
                node = std::move(manifestNode),
                treeID = std::move(edenTreeID),
                batch = std::move(writeBatch),
                commitId = std::move(commitId)](folly::Try<folly::Unit> val) {
        val.value();
        // Now try loading it again
        unionStore_->wlock()->markForRefresh();
        auto content =
            unionStoreGet(*unionStore_->wlock(), ownedPath.stringPiece(), node);
        return processTree(
            content, node, treeID, ownedPath, commitId, batch.get());
      });
}

std::unique_ptr<Tree> HgBackingStore::processTree(
    ConstantStringRef& content,
    const Hash& manifestNode,
    const Hash& edenTreeID,
    RelativePathPiece path,
    const std::optional<Hash>& commitHash,
    LocalStore::WriteBatch* writeBatch) {
  if (!content.content()) {
    // This generally shouldn't happen: the UnionDatapackStore throws on
    // error instead of returning null.  We're checking simply due to an
    // abundance of caution.
    throw std::domain_error(folly::to<std::string>(
        "HgBackingStore::importTree received null tree from mercurial store for ",
        path,
        ", ID ",
        manifestNode.toString()));
  }
  Manifest manifest(
      content, reinterpret_cast<const char*>(manifestNode.getBytes().data()));
  std::vector<TreeEntry> entries;

  auto iter = manifest.getIterator();
  while (!iter.isfinished()) {
    auto* entry = iter.currentvalue();

    // The node is the hex string representation of the hash, but
    // it is not NUL terminated!
    StringPiece node(entry->get_node(), 40);
    Hash entryHash(node);

    StringPiece entryName(entry->filename, entry->filenamelen);

    TreeEntryType fileType;

    StringPiece entryFlag;
    if (entry->flag) {
      // entry->flag is a char* but is unfortunately not nul terminated.
      // All known flag values are currently only a single character, and
      // there are never any multi-character flags.
      entryFlag.assign(entry->flag, entry->flag + 1);
    }

    XLOG(DBG9) << "tree: " << manifestNode << " " << entryName
               << " node: " << node << " flag: " << entryFlag;

    if (entry->isdirectory()) {
      fileType = TreeEntryType::TREE;
    } else if (entry->flag) {
      switch (*entry->flag) {
        case 'x':
          fileType = TreeEntryType::EXECUTABLE_FILE;
          break;
        case 'l':
          fileType = TreeEntryType::SYMLINK;
          break;
        default:
          throw std::runtime_error(folly::to<std::string>(
              "unsupported file flags for ",
              path,
              "/",
              entryName,
              ": ",
              entryFlag));
      }
    } else {
      fileType = TreeEntryType::REGULAR_FILE;
    }

    auto proxyHash = HgProxyHash::store(
        path + RelativePathPiece(entryName), entryHash, writeBatch);
    if (commitHash) {
      ScsProxyHash::store(
          proxyHash,
          path + RelativePathPiece(entryName),
          commitHash.value(),
          writeBatch);
    }

    entries.emplace_back(proxyHash, entryName, fileType);

    iter.next();
  }
  writeBatch->flush();

  return make_unique<Tree>(std::move(entries), edenTreeID);
}

folly::Future<std::unique_ptr<Tree>> HgBackingStore::importTreeManifest(
    const Hash& commitId) {
  return folly::via(
             importThreadPool_.get(),
             [commitId] {
               return getThreadLocalImporter().resolveManifestNode(
                   commitId.toString());
             })
      .via(serverThreadPool_)
      .thenValue([this, commitId](auto manifestNode) {
        XLOG(DBG2) << "revision " << commitId.toString()
                   << " has manifest node " << manifestNode;
        // Record that we are at the root for this node
        RelativePathPiece path{};
        auto proxyInfo = HgProxyHash::prepareToStore(path, manifestNode);
        // needs to write the scs proxy hash before the fetch so that it is
        // available for the request
        auto batch = localStore_->beginWrite();
        ScsProxyHash::store(proxyInfo.first, path, commitId, batch.get());
        batch->flush();
        auto futTree =
            importTreeImpl(manifestNode, proxyInfo.first, path, commitId);
        return std::move(futTree).thenValue(
            [batch = localStore_->beginWrite(),
             info = std::move(proxyInfo)](auto tree) {
              // Only write the proxy hash value for this once we've imported
              // the root.
              HgProxyHash::store(info, batch.get());
              batch->flush();
              return tree;
            });
      });
}

unique_ptr<Blob> HgBackingStore::getBlobFromHgCache(
    const Hash& id,
    const HgProxyHash& hgInfo) {
  if (auto content = datapackStore_.getBlobLocal(id, hgInfo)) {
    XLOG(DBG5) << "importing file contents of '" << hgInfo.path() << "', "
               << hgInfo.revHash().toString() << " from datapack store";
    return content;
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

SemiFuture<unique_ptr<Blob>> HgBackingStore::getBlob(
    const Hash& id,
    ObjectFetchContext& /*context*/,
    ImportPriority /* priority */) {
  folly::stop_watch<std::chrono::milliseconds> watch;
  // Look up the mercurial path and file revision hash,
  // which we need to import the data from mercurial
  HgProxyHash hgInfo(localStore_.get(), id, "importFileContents");

  if (auto result = getBlobFromHgCache(id, hgInfo)) {
    stats_->getHgBackingStoreStatsForCurrentThread()
        .hgBackingStoreGetBlob.addValue(watch.elapsed().count());
    return folly::makeSemiFuture(std::move(result));
  }

  return fetchBlobFromHgImporter(std::move(hgInfo))
      .deferValue([stats = stats_, watch](auto&& blob) {
        stats->getHgBackingStoreStatsForCurrentThread()
            .hgBackingStoreGetBlob.addValue(watch.elapsed().count());
        return std::forward<decltype(blob)>(blob);
      });
}

SemiFuture<folly::Unit> HgBackingStore::prefetchBlobs(
    const std::vector<Hash>& ids) {
  return HgProxyHash::getBatch(localStore_.get(), ids)
      .via(importThreadPool_.get())
      .thenValue([&liveImportPrefetchWatches = liveImportPrefetchWatches_](
                     std::vector<HgProxyHash>&& hgPathHashes) {
        RequestMetricsScope queueTracker{&liveImportPrefetchWatches};
        return getThreadLocalImporter().prefetchFiles(hgPathHashes);
      })
      .via(serverThreadPool_);
}

SemiFuture<unique_ptr<Tree>> HgBackingStore::getTreeForCommit(
    const Hash& commitID) {
  return localStore_
      ->getFuture(KeySpace::HgCommitToTreeFamily, commitID.getBytes())
      .thenValue(
          [this, commitID](
              StoreResult result) -> folly::SemiFuture<unique_ptr<Tree>> {
            if (!result.isValid()) {
              return importTreeForCommit(commitID);
            }

            auto rootTreeHash = Hash{result.bytes()};
            XLOG(DBG5) << "found existing tree " << rootTreeHash.toString()
                       << " for mercurial commit " << commitID.toString();
            return getTreeForRootTreeImpl(commitID, rootTreeHash);
          });
}

folly::SemiFuture<unique_ptr<Tree>> HgBackingStore::getTreeForManifest(
    const Hash& commitID,
    const Hash& manifestID) {
  // Construct the edenTreeID to pass to localStore lookup
  auto rootTreeHash =
      HgProxyHash::prepareToStore(RelativePathPiece{}, manifestID).first;
  return getTreeForRootTreeImpl(commitID, rootTreeHash).via(serverThreadPool_);
}

folly::Future<unique_ptr<Tree>> HgBackingStore::getTreeForRootTreeImpl(
    const Hash& commitID,
    const Hash& rootTreeHash) {
  return localStore_->getTree(rootTreeHash)
      .thenValue(
          [this, rootTreeHash, commitID](
              std::unique_ptr<Tree> tree) -> folly::Future<unique_ptr<Tree>> {
            if (tree) {
              return folly::makeFuture(std::move(tree));
            }

            return localStore_->getTree(rootTreeHash)
                .thenValue(
                    [this, rootTreeHash, commitID](std::unique_ptr<Tree> tree)
                        -> folly::SemiFuture<unique_ptr<Tree>> {
                      if (tree) {
                        return std::move(tree);
                      }

                      // No corresponding tree for this commit ID! Must
                      // re-import. This could happen if RocksDB is corrupted
                      // in some way or deleting entries races with
                      // population.
                      XLOG(WARN) << "No corresponding tree " << rootTreeHash
                                 << " for commit " << commitID
                                 << "; will import again";
                      return importTreeForCommit(commitID);
                    });
          });
}

folly::SemiFuture<unique_ptr<Tree>> HgBackingStore::importTreeForCommit(
    Hash commitID) {
  return importTreeManifest(commitID).thenValue(
      [this, commitID](std::unique_ptr<Tree> rootTree) {
        XLOG(DBG1) << "imported mercurial commit " << commitID.toString()
                   << " as tree " << rootTree->getHash().toString();

        localStore_->put(
            KeySpace::HgCommitToTreeFamily,
            commitID,
            rootTree->getHash().getBytes());
        return rootTree;
      });
}

folly::StringPiece HgBackingStore::stringOfHgImportObject(
    HgImportObject object) {
  switch (object) {
    case HgImportObject::BLOB:
      return "blob";
    case HgImportObject::TREE:
      return "tree";
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
  }
  EDEN_BUG() << "unknown hg import object " << enumValue(object);
}

void HgBackingStore::periodicManagementTask() {
  datapackStore_.refresh();

  if (unionStore_) {
    unionStore_->wlock()->refresh();
  }
}

} // namespace eden
} // namespace facebook
