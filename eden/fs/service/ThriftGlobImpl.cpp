/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/ThriftGlobImpl.h"

#include <folly/coro/Collect.h>
#include <folly/coro/Invoke.h>
#include <folly/coro/Task.h>
#include <folly/coro/safe/NowTask.h>
#include <folly/logging/LogLevel.h>
#include <folly/logging/xlog.h>
#include <memory>
#include "eden/common/utils/UnboundedQueueExecutor.h"
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/GlobNode.h"
#include "eden/fs/inodes/ServerState.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/model/LocalFiles.h"
#include "eden/fs/model/RootId.h"
#include "eden/fs/service/gen-cpp2/eden_types.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/store/PathLoader.h"
#include "eden/fs/utils/EdenError.h"
#include "eden/fs/utils/GlobNodeImpl.h"
#include "eden/fs/utils/GlobResult.h"
#include "eden/fs/utils/GlobTree.h"

namespace facebook::eden {

namespace {
// Compile the list of globs into a tree
void compileGlobs(const std::vector<std::string>& globs, GlobNodeImpl& root) {
  try {
    for (auto& globString : globs) {
      try {
        root.parse(globString);
      } catch (const std::domain_error& exc) {
        throw newEdenError(
            EdenErrorType::ARGUMENT_ERROR,
            "Invalid glob (",
            exc.what(),
            "): ",
            globString);
      }
    }
  } catch (const std::system_error& exc) {
    throw newEdenError(exc);
  }
}

folly::coro::now_task<std::unique_ptr<LocalFiles>> computeLocalFiles(
    const std::shared_ptr<EdenMount>& edenMount,
    const std::shared_ptr<ServerState>& serverState,
    bool includeDotfiles,
    const RootId& rootId,
    const TreeInodePtr& rootInode,
    const std::vector<std::string>& suffixGlobs,
    const ObjectFetchContextPtr& context) {
  auto enforceParents = serverState->getReloadableConfig()
                            ->getEdenConfig()
                            ->enforceParents.getValue();
  bool caseSensitive =
      serverState->getEdenConfig()->globUseMountCaseSensitivity.getValue();

  auto status = co_await edenMount->co_diff(
      rootInode,
      rootId,
      folly::CancellationToken(),
      context,
      /*listIgnored=*/true,
      enforceParents);

  // Everything below is synchronous processing
  if (!status->errors_ref().value().empty()) {
    XLOG(DBG4, "Error getting local changes");
    throw newEdenError(
        EINVAL, EdenErrorType::POSIX_ERROR, "unable to look up local files");
  }
  std::vector<GlobMatcher> globMatchers{};
  GlobOptions options =
      includeDotfiles ? GlobOptions::DEFAULT : GlobOptions::IGNORE_DOTFILES;
  if (caseSensitive) {
    if (edenMount->getCheckoutConfig()->getCaseSensitive() ==
        CaseSensitivity::Insensitive) {
      options |= GlobOptions::CASE_INSENSITIVE;
    }
  }
  for (auto& glob : suffixGlobs) {
    XLOGF(DBG4, "Creating glob matcher for glob: {}", glob);
    auto expectGlobMatcher = GlobMatcher::create("**/*" + glob, options);
    if (expectGlobMatcher.hasValue()) {
      XLOGF(DBG4, "Successfully created glob matcher for glob: {}", glob);
      globMatchers.push_back(expectGlobMatcher.value());
    } else {
      XLOGF(ERR, "Invalid glob: {}", glob);
    }
  }

  std::unique_ptr<LocalFiles> localFiles = std::make_unique<LocalFiles>();
  for (auto const& [pathString, scmFileStatus] :
       status->entries_ref().value()) {
    if (scmFileStatus == ScmFileStatus::ADDED) {
      for (auto& matcher : globMatchers) {
        if (matcher.match(pathString)) {
          localFiles->addedFiles.insert(pathString);
        }
      }
    } else if (scmFileStatus == ScmFileStatus::REMOVED) {
      localFiles->removedFiles.insert(pathString);
    } else if (scmFileStatus == ScmFileStatus::MODIFIED) {
      for (auto& matcher : globMatchers) {
        if (matcher.match(pathString)) {
          localFiles->modifiedFiles.insert(pathString);
        }
      }
    } else if (scmFileStatus == ScmFileStatus::IGNORED) {
      localFiles->ignoredFiles.insert(pathString);
    }
  }
  co_return localFiles;
}

} // namespace

ThriftGlobImpl::ThriftGlobImpl(const GlobParams& params)
    : includeDotfiles_{*params.includeDotfiles()},
      prefetchFiles_{*params.prefetchFiles()},
      suppressFileList_{*params.suppressFileList()},
      wantDtype_{*params.wantDtype()},
      listOnlyFiles_{*params.listOnlyFiles()},
      rootIds_{*params.revisions()},
      searchRootUser_{*params.searchRoot()} {}

ThriftGlobImpl::ThriftGlobImpl(
    const PrefetchParams& params,
    bool prefetchOptimizations)
    : includeDotfiles_{true},
      prefetchFiles_{!*params.directoriesOnly()},
      suppressFileList_{
          prefetchOptimizations && !*params.returnPrefetchedFiles()},
      rootIds_{*params.revisions()},
      searchRootUser_{*params.searchRoot()} {}

ImmediateFuture<std::unique_ptr<Glob>> ThriftGlobImpl::glob(
    std::shared_ptr<EdenMount> edenMount,
    std::shared_ptr<ServerState> serverState,
    std::vector<std::string> globs,
    const ObjectFetchContextPtr& fetchContext) {
  // Move *this into the lambda so it outlives the coroutine even if the caller
  // destroys its globber after glob() returns.
  return ImmediateFuture{
      // @lint-ignore CLANGTIDY facebook-folly-coro-return-captures-local-var
      folly::coro::co_invoke(
          [globber = std::move(*this)](
              auto edenMount,
              auto serverState,
              auto globs,
              auto fetchContext) mutable
              -> folly::coro::Task<std::unique_ptr<Glob>> {
            co_return co_await globber.co_glob(
                std::move(edenMount),
                std::move(serverState),
                std::move(globs),
                std::move(fetchContext));
          },
          std::move(edenMount),
          std::move(serverState),
          std::move(globs),
          fetchContext.copy())
          .semi()};
}

folly::coro::now_task<std::unique_ptr<Glob>> ThriftGlobImpl::co_glob(
    std::shared_ptr<EdenMount> edenMount,
    std::shared_ptr<ServerState> serverState,
    std::vector<std::string> globs,
    const ObjectFetchContextPtr& fetchContext) {
  bool prefetchOptimizations =
      serverState->getEdenConfig()->prefetchOptimizations.getValue();
  bool dedupePrefetchFiles =
      serverState->getEdenConfig()->globDedupePrefetchFiles.getValue() ||
      !prefetchOptimizations;

  auto fileBlobsToPrefetch =
      prefetchFiles_ ? std::make_shared<PrefetchList>() : nullptr;

  // These ids must outlive the GlobResult created by evaluate as the
  // GlobResults will hold on to references to these ids
  auto originRootIds = std::make_unique<std::vector<RootId>>();

  // Globs will be evaluated against the specified commits or the current commit
  // if none are specified. The results will be collected here.
  std::vector<folly::coro::Task<void>> globTasks{};
  auto globResults = std::make_shared<ResultList>();

  RelativePath searchRoot;
  if (!(searchRootUser_.empty() || searchRootUser_ == ".")) {
    searchRoot = RelativePath{searchRootUser_};
  }
  std::shared_ptr<GlobTree> globTree = nullptr;
  std::shared_ptr<GlobNode> globNode = nullptr;

  if (!rootIds_.empty()) {
    // Note that we MUST reserve here, otherwise while emplacing we might
    // invalidate the earlier commitId references
    globTasks.reserve(rootIds_.size());
    originRootIds->reserve(rootIds_.size());
    auto caseSensitivity =
        serverState->getEdenConfig()->globUseMountCaseSensitivity.getValue()
        ? edenMount->getCheckoutConfig()->getCaseSensitive()
        : CaseSensitivity::Sensitive;
    globTree = std::make_shared<GlobTree>(
        bool(includeDotfiles_),
        caseSensitivity,
        bool(prefetchOptimizations),
        serverState->getEdenConfig()->globRecursiveAsyncDepth.getValue());
    compileGlobs(globs, *globTree);
    for (auto& rootId : rootIds_) {
      const RootId& originRootId = originRootIds->emplace_back(
          edenMount->getObjectStore()->parseRootId(rootId));

      globTasks.emplace_back(
          folly::coro::co_invoke(
              [edenMount,
               globTree,
               fetchContext = fetchContext.copy(),
               searchRoot,
               fileBlobsToPrefetch,
               globResults,
               &originRootId,
               prefetchOptimizations,
               suppressFileList =
                   suppressFileList_]() mutable -> folly::coro::Task<void> {
                auto rootTree =
                    co_await edenMount->getObjectStore()->co_getRootTree(
                        originRootId, fetchContext);
                auto tree = co_await resolveTree(
                    *edenMount->getObjectStore(),
                    fetchContext,
                    std::move(rootTree.tree),
                    searchRoot);
                co_await globTree->evaluate(
                    edenMount->getObjectStore(),
                    fetchContext,
                    RelativePathPiece(),
                    std::move(tree),
                    fileBlobsToPrefetch.get(),
                    suppressFileList && prefetchOptimizations
                        ? nullptr
                        : globResults.get(),
                    originRootId);
              }));
    }
  } else {
    bool includeDotfiles = includeDotfiles_;
    CaseSensitivity caseSensitive =
        serverState->getEdenConfig()->globUseMountCaseSensitivity.getValue()
        ? edenMount->getCheckoutConfig()->getCaseSensitive()
        : CaseSensitivity::Sensitive;
    uint32_t asyncDepth =
        serverState->getEdenConfig()->globRecursiveAsyncDepth.getValue();
    globNode = std::make_shared<GlobNode>(
        includeDotfiles,
        caseSensitive,
        bool(prefetchOptimizations),
        asyncDepth);
    compileGlobs(globs, *globNode);
    const RootId& originRootId =
        originRootIds->emplace_back(edenMount->getCheckedOutRootId());
    globTasks.emplace_back(
        folly::coro::co_invoke(
            [fetchContext = fetchContext.copy(),
             globNode,
             edenMount,
             fileBlobsToPrefetch,
             globResults,
             &originRootId,
             prefetchOptimizations,
             searchRoot,
             suppressFileList =
                 suppressFileList_]() mutable -> folly::coro::Task<void> {
              auto inode =
                  co_await edenMount->co_getInodeSlow(searchRoot, fetchContext);
              co_await globNode->evaluate(
                  edenMount->getObjectStore(),
                  fetchContext,
                  RelativePathPiece(),
                  inode.asTreePtr(),
                  fileBlobsToPrefetch.get(),
                  suppressFileList && prefetchOptimizations ? nullptr
                                                            : globResults.get(),
                  originRootId);
            }));
  }

  // Copy member fields before suspension point — the ThriftGlobImpl object
  // may be destroyed while co_await suspends (it's owned by the caller's
  // lambda which returns the ImmediateFuture before co_glob completes).
  bool suppressFileList = suppressFileList_;
  bool listOnlyFiles = listOnlyFiles_;
  bool wantDtype = wantDtype_;
  size_t numRevisions = rootIds_.size();

  // When there are 0 or 1 revisions, every entry has the same origin hash.
  // Skip the per-file renderRootId() call and the resulting list, as no
  // caller can use it to distinguish between revisions.
  bool populateOriginHashes = numRevisions > 1 ||
      !serverState->getEdenConfig()->globSkipRedundantOriginHashes.getValue();

  // Note: we use collectAllTryRange() rather than collectAllRange() here
  // because collectAllRange() sends cooperative cancellation to sibling
  // tasks on first failure, which could cause incomplete writes to
  // globResults/fileBlobsToPrefetch. collectAllTryRange() lets all tasks
  // run to completion before propagating errors.
  auto globTries =
      co_await folly::coro::collectAllTryRange(std::move(globTasks));

  // Post-process: sort and dedup results
  std::vector<GlobResult> sortedResults;
  if (!suppressFileList) {
    std::swap(sortedResults, *globResults->wlock());
    for (auto& t : globTries) {
      t.throwUnlessValue();
    }
    std::sort(sortedResults.begin(), sortedResults.end());
    auto resultsNewEnd =
        std::unique(sortedResults.begin(), sortedResults.end());
    sortedResults.erase(resultsNewEnd, sortedResults.end());
  }

  // Deduplicate files as an optimization. The BackingStore does not
  // necessarily dedupe fetches (although SaplingBackingStore does
  // in the scmstore::FileStore, per batch).
  //
  // Note that normally duplicates are rare, and deduping a list of
  // millions of files can take 5s, so it typically is not worth it.
  if (dedupePrefetchFiles && fileBlobsToPrefetch) {
    auto fileBlobsToPrefetchLocked = fileBlobsToPrefetch->wlock();
    folly::F14FastSet<ObjectId> seen;
    size_t i = 0;
    while (i < fileBlobsToPrefetchLocked->size()) {
      if (seen.insert((*fileBlobsToPrefetchLocked)[i]).second) {
        ++i;
      } else {
        std::swap(
            (*fileBlobsToPrefetchLocked)[i], fileBlobsToPrefetchLocked->back());
        fileBlobsToPrefetchLocked->pop_back();
      }
    }
  }

  // Build the output
  auto out = std::make_unique<Glob>();

  if (!suppressFileList) {
    for (auto& entry : sortedResults) {
      if (!listOnlyFiles || entry.dtype != dtype_t::Dir) {
        out->matchingFiles()->emplace_back(entry.name.asString());

        if (wantDtype) {
          out->dtypes()->emplace_back(static_cast<OsDtype>(entry.dtype));
        }

        if (populateOriginHashes) {
          out->originHashes()->emplace_back(
              edenMount->getObjectStore()->renderRootId(*entry.originId));
        }
      }
    }
  }

  // Prefetch blobs in parallel batches
  if (fileBlobsToPrefetch) {
    auto store = edenMount->getObjectStore();
    auto blobs = fileBlobsToPrefetch->rlock();
    auto range = folly::Range{blobs->data(), blobs->size()};

    std::vector<folly::coro::Task<void>> prefetchTasks;
    while (range.size() > 20480) {
      auto curRange = range.subpiece(0, 20480);
      range.advance(20480);
      prefetchTasks.emplace_back(
          folly::coro::co_invoke(
              [store, curRange, fetchContext = fetchContext.copy()]()
                  -> folly::coro::Task<void> {
                co_await folly::coro::co_reschedule_on_current_executor;
                co_await store->prefetchBlobs(curRange, fetchContext);
              }));
    }
    if (!range.empty()) {
      prefetchTasks.emplace_back(
          folly::coro::co_invoke(
              [store, range, fetchContext = fetchContext.copy()]()
                  -> folly::coro::Task<void> {
                co_await folly::coro::co_reschedule_on_current_executor;
                co_await store->prefetchBlobs(range, fetchContext);
              }));
    }
    // Use collectAllTryRange (not collectAllRange) to avoid cooperative
    // cancellation on failure. Prefetch errors are silently discarded
    // to match the futures glob implementation — partial prefetching
    // is still useful.
    co_await folly::coro::collectAllTryRange(std::move(prefetchTasks));
  }

  co_return out;
}

folly::coro::now_task<std::vector<BackingStore::GetGlobFilesResult>>
getLocalGlobResults(
    const std::shared_ptr<EdenMount>& edenMount,
    const std::shared_ptr<ServerState>& serverState,
    bool includeDotfiles,
    const std::vector<std::string>& suffixGlobs,
    const std::vector<std::string>& prefixes,
    const TreeInodePtr& rootInode,
    const ObjectFetchContextPtr& context) {
  XLOG(DBG3, "No commit id in input, using current id");
  auto rootId = edenMount->getCheckedOutRootId();
  auto& store = edenMount->getObjectStore();

  auto remoteGlobFiles =
      co_await store->co_getGlobFiles(rootId, suffixGlobs, prefixes, context);

  auto localFiles = co_await computeLocalFiles(
      edenMount,
      serverState,
      includeDotfiles,
      rootId,
      rootInode,
      suffixGlobs,
      context);

  BackingStore::GetGlobFilesResult filteredRemoteGlobFiles;
  filteredRemoteGlobFiles.rootId = remoteGlobFiles.rootId;
  for (auto& entry : remoteGlobFiles.globFiles) {
    if (localFiles->removedFiles.contains(entry) ||
        localFiles->addedFiles.contains(entry) ||
        localFiles->modifiedFiles.contains(entry)) {
      continue;
    }
    filteredRemoteGlobFiles.globFiles.emplace_back(entry);
  }

  BackingStore::GetGlobFilesResult localGlobFiles;
  localGlobFiles.isLocal = true;
  localGlobFiles.rootId = rootId;
  for (auto& entry : localFiles->addedFiles) {
    localGlobFiles.globFiles.emplace_back(entry);
  }
  for (auto& entry : localFiles->modifiedFiles) {
    localGlobFiles.globFiles.emplace_back(entry);
  }

  co_return std::vector<BackingStore::GetGlobFilesResult>{
      filteredRemoteGlobFiles, localGlobFiles};
}

std::string ThriftGlobImpl::logString() {
  return fmt::format(
      "ThriftGlobImpl {{ includeDotFiles={}, prefetchFiles={}, suppressFileList={}, wantDtype={}, listOnlyFiles={}, rootIds={}, searchRootUser={} }}",
      includeDotfiles_,
      prefetchFiles_,
      suppressFileList_,
      wantDtype_,
      listOnlyFiles_,
      fmt::join(rootIds_, ", "),
      searchRootUser_);
}

std::string ThriftGlobImpl::logString(
    const std::vector<std::string>& globs) const {
  return fmt::format(
      "ThriftGlobImpl {{ globs={}, includeDotFiles={}, prefetchFiles={}, suppressFileList={}, wantDtype={}, listOnlyFiles={}, rootIds={}, searchRootUser={} }}",
      fmt::join(globs, ", "),
      includeDotfiles_,
      prefetchFiles_,
      suppressFileList_,
      wantDtype_,
      listOnlyFiles_,
      fmt::join(rootIds_, ", "),
      searchRootUser_);
}

} // namespace facebook::eden
