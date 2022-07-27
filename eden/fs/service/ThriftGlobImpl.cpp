/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/ThriftGlobImpl.h"

#include <folly/futures/Future.h>
#include <folly/logging/LogLevel.h>
#include <folly/logging/xlog.h>
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/GlobNode.h"
#include "eden/fs/inodes/ServerState.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/model/RootId.h"
#include "eden/fs/service/gen-cpp2/eden_types.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/store/PathLoader.h"
#include "eden/fs/utils/EdenError.h"
#include "eden/fs/utils/UnboundedQueueExecutor.h"

namespace facebook::eden {

ThriftGlobImpl::ThriftGlobImpl(const GlobParams& params)
    : includeDotfiles_{*params.includeDotfiles_ref()},
      prefetchFiles_{*params.prefetchFiles_ref()},
      suppressFileList_{*params.suppressFileList_ref()},
      wantDtype_{*params.wantDtype_ref()},
      listOnlyFiles_{*params.listOnlyFiles_ref()},
      rootHashes_{*params.revisions_ref()},
      searchRootUser_{*params.searchRoot_ref()} {}

ImmediateFuture<std::unique_ptr<Glob>> ThriftGlobImpl::glob(
    std::shared_ptr<EdenMount> edenMount,
    std::shared_ptr<ServerState> serverState,
    std::vector<std::string> globs,
    ObjectFetchContext& fetchContext) {
  // Compile the list of globs into a tree
  auto globRoot = std::make_shared<GlobNode>(includeDotfiles_);
  try {
    for (auto& globString : globs) {
      try {
        globRoot->parse(globString);
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

  auto fileBlobsToPrefetch =
      prefetchFiles_ ? std::make_shared<GlobNode::PrefetchList>() : nullptr;

  // These hashes must outlive the GlobResult created by evaluate as the
  // GlobResults will hold on to references to these hashes
  auto originRootIds = std::make_unique<std::vector<RootId>>();

  // Globs will be evaluated against the specified commits or the current commit
  // if none are specified. The results will be collected here.
  std::vector<ImmediateFuture<folly::Unit>> globFutures{};
  auto globResults = std::make_shared<GlobNode::ResultList>();

  RelativePath searchRoot;
  if (!(searchRootUser_.empty() || searchRootUser_ == ".")) {
    searchRoot = RelativePath{searchRootUser_};
  }

  if (!rootHashes_.empty()) {
    // Note that we MUST reserve here, otherwise while emplacing we might
    // invalidate the earlier commitHash refrences
    globFutures.reserve(rootHashes_.size());
    originRootIds->reserve(rootHashes_.size());
    for (auto& rootHash : rootHashes_) {
      const RootId& originRootId = originRootIds->emplace_back(
          edenMount->getObjectStore()->parseRootId(rootHash));

      globFutures.emplace_back(
          edenMount->getObjectStore()
              ->getRootTree(originRootId, fetchContext)
              .thenValue([edenMount, globRoot, &fetchContext, searchRoot](
                             std::shared_ptr<const Tree>&& rootTree) {
                return resolveTree(
                    *edenMount->getObjectStore(),
                    fetchContext,
                    std::move(rootTree),
                    searchRoot);
              })
              .thenValue(
                  [edenMount,
                   globRoot,
                   &fetchContext,
                   fileBlobsToPrefetch,
                   globResults,
                   &originRootId](std::shared_ptr<const Tree>&& tree) mutable {
                    return globRoot->evaluate(
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
    const RootId& originRootId =
        originRootIds->emplace_back(edenMount->getCheckedOutRootId());
    globFutures.emplace_back(
        edenMount->getInodeSlow(searchRoot, fetchContext)
            .thenValue([&fetchContext,
                        globRoot,
                        edenMount,
                        fileBlobsToPrefetch,
                        globResults,
                        &originRootId](InodePtr inode) mutable {
              return globRoot->evaluate(
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
            std::vector<GlobNode::GlobResult> sortedResults;
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
          .thenValue([edenMount,
                      wantDtype = wantDtype_,
                      fileBlobsToPrefetch,
                      suppressFileList = suppressFileList_,
                      listOnlyFiles = listOnlyFiles_,
                      &fetchContext,
                      config = serverState->getEdenConfig()](
                         std::vector<GlobNode::GlobResult>&& results) mutable {
            auto out = std::make_unique<Glob>();

            if (!suppressFileList) {
              // already deduplicated at this point, no need to de-dup
              for (auto& entry : results) {
                if (!listOnlyFiles || entry.dtype != dtype_t::Dir) {
                  out->matchingFiles_ref()->emplace_back(
                      entry.name.stringPiece().toString());

                  if (wantDtype) {
                    out->dtypes_ref()->emplace_back(
                        static_cast<OsDtype>(entry.dtype));
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
                futures.emplace_back(store->prefetchBlobs(range, fetchContext));
              }

              return collectAll(std::move(futures))
                  .thenValue([glob = std::move(out), fileBlobsToPrefetch](
                                 auto&&) mutable { return std::move(glob); });
            }
            return ImmediateFuture{std::move(out)};
          })
          .ensure([globRoot, originRootIds = std::move(originRootIds)]() {
            // keep globRoot and originRootIds alive until the end
          });

  return prefetchFuture;
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

} // namespace facebook::eden
