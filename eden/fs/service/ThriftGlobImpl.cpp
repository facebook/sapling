/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/ThriftGlobImpl.h"

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

ImmediateFuture<std::unique_ptr<LocalFiles>> computeLocalFiles(
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

  return edenMount
      ->diff(
          rootInode,
          rootId,
          // Default uncancellable token
          folly::CancellationToken(),
          context,
          /*listIgnored=*/true,
          enforceParents)
      .thenValue([rootId,
                  edenMount,
                  caseSensitive,
                  suffixGlobs,
                  includeDotfiles](auto&& status) {
        if (!status->errors_ref().value().empty()) {
          XLOG(DBG4) << "Error getting local changes";
          throw newEdenError(
              EINVAL,
              EdenErrorType::POSIX_ERROR,
              "unable to look up local files");
        }
        std::vector<GlobMatcher> globMatchers{};
        GlobOptions options = includeDotfiles ? GlobOptions::DEFAULT
                                              : GlobOptions::IGNORE_DOTFILES;
        if (caseSensitive) {
          if (edenMount->getCheckoutConfig()->getCaseSensitive() ==
              CaseSensitivity::Insensitive) {
            options |= GlobOptions::CASE_INSENSITIVE;
          }
        }
        for (auto& glob : suffixGlobs) {
          XLOG(DBG4) << "Creating glob matcher for glob: " << glob;
          auto expectGlobMatcher = GlobMatcher::create("**/*" + glob, options);
          if (expectGlobMatcher.hasValue()) {
            XLOG(DBG4) << "Successfully created glob matcher for glob: " << glob;
            globMatchers.push_back(expectGlobMatcher.value());
          } else {
            XLOG(ERR) << "Invalid glob: " << glob;
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
            // Globbing is not applied on non-added files
            // since they'll use the globbed results from
            // the server vs a set which should be faster
            // than globbing every change
          } else if (scmFileStatus == ScmFileStatus::REMOVED) {
            // Don't return files that have been deleted
            // locally
            localFiles->removedFiles.insert(pathString);
          } else if (scmFileStatus == ScmFileStatus::MODIFIED) {
            for (auto& matcher : globMatchers) {
              if (matcher.match(pathString)) {
                localFiles->modifiedFiles.insert(pathString);
              }
            }
          } else if (scmFileStatus == ScmFileStatus::IGNORED) {
            // Not doing anything with these for now, just putting
            // it here for completeness
            localFiles->ignoredFiles.insert(pathString);
          }
        }
        return localFiles;
      });
}
} // namespace

ThriftGlobImpl::ThriftGlobImpl(const GlobParams& params)
    : includeDotfiles_{*params.includeDotfiles_ref()},
      prefetchFiles_{*params.prefetchFiles_ref()},
      suppressFileList_{*params.suppressFileList_ref()},
      wantDtype_{*params.wantDtype_ref()},
      listOnlyFiles_{*params.listOnlyFiles_ref()},
      rootHashes_{*params.revisions_ref()},
      searchRootUser_{*params.searchRoot_ref()} {}

ThriftGlobImpl::ThriftGlobImpl(const PrefetchParams& params)
    : includeDotfiles_{true},
      prefetchFiles_{!*params.directoriesOnly_ref()},
      rootHashes_{*params.revisions_ref()},
      searchRootUser_{*params.searchRoot_ref()} {}

ImmediateFuture<std::unique_ptr<Glob>> ThriftGlobImpl::glob(
    std::shared_ptr<EdenMount> edenMount,
    std::shared_ptr<ServerState> serverState,
    std::vector<std::string> globs,
    const ObjectFetchContextPtr& fetchContext) {
  bool windowsSymlinksEnabled =
      edenMount->getCheckoutConfig()->getEnableWindowsSymlinks();

  auto fileBlobsToPrefetch =
      prefetchFiles_ ? std::make_shared<PrefetchList>() : nullptr;

  // These hashes must outlive the GlobResult created by evaluate as the
  // GlobResults will hold on to references to these hashes
  auto originRootIds = std::make_unique<std::vector<RootId>>();

  // Globs will be evaluated against the specified commits or the current commit
  // if none are specified. The results will be collected here.
  std::vector<ImmediateFuture<folly::Unit>> globFutures{};
  auto globResults = std::make_shared<ResultList>();

  RelativePath searchRoot;
  if (!(searchRootUser_.empty() || searchRootUser_ == ".")) {
    searchRoot = RelativePath{searchRootUser_};
  }
  std::shared_ptr<GlobTree> globTree = nullptr;
  std::shared_ptr<GlobNode> globNode = nullptr;

  if (!rootHashes_.empty()) {
    // Note that we MUST reserve here, otherwise while emplacing we might
    // invalidate the earlier commitHash references
    globFutures.reserve(rootHashes_.size());
    originRootIds->reserve(rootHashes_.size());
    globTree = std::make_shared<GlobTree>(
        includeDotfiles_,
        serverState->getEdenConfig()->globUseMountCaseSensitivity.getValue()
            ? edenMount->getCheckoutConfig()->getCaseSensitive()
            : CaseSensitivity::Sensitive);
    compileGlobs(globs, *globTree);
    for (auto& rootHash : rootHashes_) {
      const RootId& originRootId = originRootIds->emplace_back(
          edenMount->getObjectStore()->parseRootId(rootHash));

      globFutures.emplace_back(
          edenMount->getObjectStore()
              ->getRootTree(originRootId, fetchContext)
              .thenValue([edenMount,
                          globTree,
                          fetchContext = fetchContext.copy(),
                          searchRoot](ObjectStore::GetRootTreeResult rootTree) {
                return resolveTree(
                    *edenMount->getObjectStore(),
                    fetchContext,
                    std::move(rootTree.tree),
                    searchRoot);
              })
              .thenValue(
                  [edenMount,
                   globTree,
                   fetchContext = fetchContext.copy(),
                   fileBlobsToPrefetch,
                   globResults,
                   &originRootId](std::shared_ptr<const Tree>&& tree) mutable {
                    return globTree->evaluate(
                        edenMount->getObjectStore(),
                        fetchContext,
                        RelativePathPiece(),
                        std::move(tree),
                        fileBlobsToPrefetch.get(),
                        *globResults,
                        originRootId);
                  }));
    }
  } else {
    globNode = std::make_shared<GlobNode>(
        includeDotfiles_,
        serverState->getEdenConfig()->globUseMountCaseSensitivity.getValue()
            ? edenMount->getCheckoutConfig()->getCaseSensitive()
            : CaseSensitivity::Sensitive);
    compileGlobs(globs, *globNode);
    const RootId& originRootId =
        originRootIds->emplace_back(edenMount->getCheckedOutRootId());
    globFutures.emplace_back(
        edenMount->getInodeSlow(searchRoot, fetchContext)
            .thenValue([fetchContext = fetchContext.copy(),
                        globNode,
                        edenMount,
                        fileBlobsToPrefetch,
                        globResults,
                        &originRootId](InodePtr inode) mutable {
              return globNode->evaluate(
                  edenMount->getObjectStore(),
                  fetchContext,
                  RelativePathPiece(),
                  inode.asTreePtr(),
                  fileBlobsToPrefetch.get(),
                  *globResults,
                  originRootId);
            }));
  }

  auto prefetchFuture =
      collectAll(std::move(globFutures))
          .thenValue([fileBlobsToPrefetch,
                      globResults = std::move(globResults),
                      suppressFileList = suppressFileList_](
                         std::vector<folly::Try<folly::Unit>>&& tries) {
            std::vector<GlobResult> sortedResults;
            if (!suppressFileList) {
              std::swap(sortedResults, *globResults->wlock());
              for (auto& try_ : tries) {
                try_.throwUnlessValue();
              }
              std::sort(sortedResults.begin(), sortedResults.end());
              auto resultsNewEnd =
                  std::unique(sortedResults.begin(), sortedResults.end());
              sortedResults.erase(resultsNewEnd, sortedResults.end());
            }

            // fileBlobsToPrefetch is deduplicated as an optimization.
            // The BackingStore layer does not deduplicate fetches, so lets
            // avoid causing too many duplicates here.
            if (fileBlobsToPrefetch) {
              auto fileBlobsToPrefetchLocked = fileBlobsToPrefetch->wlock();
              std::sort(
                  fileBlobsToPrefetchLocked->begin(),
                  fileBlobsToPrefetchLocked->end(),
                  std::less<ObjectId>{});
              auto fileBlobsToPrefetchNewEnd = std::unique(
                  fileBlobsToPrefetchLocked->begin(),
                  fileBlobsToPrefetchLocked->end(),
                  std::equal_to<ObjectId>());
              fileBlobsToPrefetchLocked->erase(
                  fileBlobsToPrefetchNewEnd, fileBlobsToPrefetchLocked->end());
            }

            return sortedResults;
          })
          .thenValue(
              [edenMount,
               wantDtype = wantDtype_,
               fileBlobsToPrefetch,
               suppressFileList = suppressFileList_,
               listOnlyFiles = listOnlyFiles_,
               fetchContext = fetchContext.copy(),
               windowsSymlinksEnabled = windowsSymlinksEnabled,
               config = serverState->getEdenConfig()](
                  std::vector<GlobResult>&& results) mutable
              -> ImmediateFuture<std::unique_ptr<Glob>> {
                auto out = std::make_unique<Glob>();

                if (!suppressFileList) {
                  // already deduplicated at this point, no need to de-dup
                  for (auto& entry : results) {
                    if (!listOnlyFiles || entry.dtype != dtype_t::Dir) {
                      out->matchingFiles_ref()->emplace_back(
                          entry.name.asString());

                      if (wantDtype) {
                        auto dtype = entry.dtype;
                        if (folly::kIsWindows && dtype == dtype_t::Symlink &&
                            !windowsSymlinksEnabled) {
                          dtype = dtype_t::Regular;
                        }
                        out->dtypes_ref()->emplace_back(
                            static_cast<OsDtype>(dtype));
                      }

                      out->originHashes_ref()->emplace_back(
                          edenMount->getObjectStore()->renderRootId(
                              *entry.originHash));
                    }
                  }
                }
                if (fileBlobsToPrefetch) {
                  std::vector<ImmediateFuture<folly::Unit>> futures;

                  auto store = edenMount->getObjectStore();
                  auto blobs = fileBlobsToPrefetch->rlock();
                  auto range = folly::Range{blobs->data(), blobs->size()};

                  while (range.size() > 20480) {
                    auto curRange = range.subpiece(0, 20480);
                    range.advance(20480);
                    futures.emplace_back(
                        store->prefetchBlobs(curRange, fetchContext));
                  }
                  if (!range.empty()) {
                    futures.emplace_back(
                        store->prefetchBlobs(range, fetchContext));
                  }

                  return collectAll(std::move(futures))
                      .thenValue([glob = std::move(out),
                                  fileBlobsToPrefetch](auto&&) mutable {
                        return std::move(glob);
                      });
                }
                return std::move(out);
              })
          .ensure(
              [globTree, globNode, originRootIds = std::move(originRootIds)]() {
                // keep globRoot and originRootIds alive until the end
              });

  return prefetchFuture;
}

ImmediateFuture<std::vector<BackingStore::GetGlobFilesResult>>
getLocalGlobResults(
    const std::shared_ptr<EdenMount>& edenMount,
    const std::shared_ptr<ServerState>& serverState,
    bool includeDotfiles,
    const std::vector<std::string>& suffixGlobs,
    const std::vector<std::string>& prefixes,
    const TreeInodePtr& rootInode,
    const ObjectFetchContextPtr& context) {
  // Use current commit hash
  XLOG(DBG3) << "No commit hash in input, using current hash";
  auto rootId = edenMount->getCheckedOutRootId();
  auto& store = edenMount->getObjectStore();
  return store->getGlobFiles(rootId, suffixGlobs, prefixes, context)
      .thenValue([edenMount,
                  serverState,
                  includeDotfiles,
                  rootId,
                  rootInode,
                  suffixGlobs,
                  context = context.copy()](auto&& remoteGlobFiles) mutable {
        return computeLocalFiles(
                   edenMount,
                   serverState,
                   includeDotfiles,
                   rootId,
                   rootInode,
                   suffixGlobs,
                   context)
            .thenValue([remoteGlobFiles = std::move(remoteGlobFiles),
                        rootId](std::unique_ptr<LocalFiles>&& localFiles) {
              BackingStore::GetGlobFilesResult filteredRemoteGlobFiles;
              filteredRemoteGlobFiles.rootId = remoteGlobFiles.rootId;
              for (auto& entry : remoteGlobFiles.globFiles) {
                if (localFiles->removedFiles.count(entry) == 1 ||
                    localFiles->addedFiles.count(entry) == 1 ||
                    localFiles->modifiedFiles.count(entry) == 1) {
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
              return std::vector<BackingStore::GetGlobFilesResult>{
                  filteredRemoteGlobFiles, localGlobFiles};
            });
      });
}

std::string ThriftGlobImpl::logString() {
  return fmt::format(
      "ThriftGlobImpl {{ includeDotFiles={}, prefetchFiles={}, suppressFileList={}, wantDtype={}, listOnlyFiles={}, rootHashes={}, searchRootUser={} }}",
      includeDotfiles_,
      prefetchFiles_,
      suppressFileList_,
      wantDtype_,
      listOnlyFiles_,
      fmt::join(rootHashes_, ", "),
      searchRootUser_);
}

std::string ThriftGlobImpl::logString(
    const std::vector<std::string>& globs) const {
  return fmt::format(
      "ThriftGlobImpl {{ globs={}, includeDotFiles={}, prefetchFiles={}, suppressFileList={}, wantDtype={}, listOnlyFiles={}, rootHashes={}, searchRootUser={} }}",
      fmt::join(globs, ", "),
      includeDotfiles_,
      prefetchFiles_,
      suppressFileList_,
      wantDtype_,
      listOnlyFiles_,
      fmt::join(rootHashes_, ", "),
      searchRootUser_);
}

} // namespace facebook::eden
