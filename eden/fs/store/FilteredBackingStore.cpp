/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/FilteredBackingStore.h"

#include <folly/Varint.h>
#include <folly/coro/Collect.h>
#include <folly/coro/Invoke.h>
#include <stdexcept>
#include <tuple>

#include "eden/common/utils/ImmediateFuture.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/ObjectId.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/BackingStore.h"
#include "eden/fs/store/filter/Filter.h"
#include "eden/fs/store/filter/FilteredObjectId.h"
#include "eden/fs/store/sl/SaplingBackingStore.h"
#include "eden/fs/utils/FilterUtils.h"

namespace facebook::eden {

FilteredBackingStore::FilteredBackingStore(
    std::shared_ptr<BackingStore> backingStore,
    std::unique_ptr<Filter> filter,
    std::shared_ptr<ReloadableConfig> config,
    bool optimizeUnfilteredTrees)
    : backingStore_{std::move(backingStore)},
      config_{std::move(config)},
      optimizeUnfilteredTrees_{optimizeUnfilteredTrees},
      filter_{std::move(filter)} {
  isSaplingBackingStore_ =
      dynamic_cast<SaplingBackingStore*>(backingStore_.get()) != nullptr;
}

FilteredBackingStore::~FilteredBackingStore() = default;

ImmediateFuture<ObjectComparison>
FilteredBackingStore::pathAffectedByFilterChange(
    RelativePathPiece pathOne,
    RelativePathPiece pathTwo,
    folly::StringPiece filterIdOne,
    folly::StringPiece filterIdTwo) {
  std::vector<ImmediateFuture<FilterCoverage>> futures;
  futures.emplace_back(filter_->getFilterCoverageForPath(pathOne, filterIdOne));
  futures.emplace_back(filter_->getFilterCoverageForPath(pathTwo, filterIdTwo));
  return collectAll(std::move(futures))
      .thenValue([](std::vector<folly::Try<FilterCoverage>>&& isFilteredVec) {
        // If we're unable to get the results from either future, we throw.
        if (!isFilteredVec[0].hasValue() || !isFilteredVec[1].hasValue()) {
          throw std::runtime_error{fmt::format(
              "Unable to determine if paths were affected by filter change: {}",
              isFilteredVec[0].hasException()
                  ? isFilteredVec[0].exception().what()
                  : isFilteredVec[1].exception().what())};
        }

        // If the FilterCoverage of both filters is the same, then there's a
        // chance the two objects are identical.
        if (isFilteredVec[0].value() == isFilteredVec[1].value()) {
          // We can only be certain that the two objects are identical if both
          // paths are RECURSIVELY filtered/unfiltered. If they aren't
          // RECURSIVELY covered, then some child may differ in coverage.
          if (isFilteredVec[0].value() != FilterCoverage::UNFILTERED) {
            return ObjectComparison::Identical;
          } else {
            return ObjectComparison::Unknown;
          }
        }

        // If we hit this path, we know the paths differ in coverage type. We
        // can guarantee that they're different.
        return ObjectComparison::Different;
      });
}

bool FilteredBackingStore::isSlOid(const ObjectId& oid) const {
  if (!isSaplingBackingStore_) {
    return false;
  }

  return SlOid::hasValidType(oid);
}

ObjectComparison FilteredBackingStore::compareObjectsById(
    const ObjectId& one,
    const ObjectId& two) {
  // If the two objects have the same bytes, then they are using the same
  // filter and must be equal.
  if (one == two) {
    return ObjectComparison::Identical;
  }

  bool oneIsSlOid = isSlOid(one);
  bool twoIsSlOid = isSlOid(two);
  if (oneIsSlOid && twoIsSlOid) {
    // Both ids are "raw" unfiltered backingstore ids - delegate to underlying
    // backingstore.
    return backingStore_->compareObjectsById(one, two);
  } else if (oneIsSlOid || twoIsSlOid) {
    // One id is an unfiltered backingstore id, the other is filtered.
    auto slOid = oneIsSlOid ? one : two;
    auto filteredOid = FilteredObjectId::fromObjectId(oneIsSlOid ? two : one);
    auto type = filteredOid.objectType();
    if (type == FilteredObjectIdType::OBJECT_TYPE_BLOB ||
        type == FilteredObjectIdType::OBJECT_TYPE_UNFILTERED_TREE) {
      // Blob and unfiltered trees both just use "normal" comparison, so fall
      // back to underlying backing store to perform that comparison.
      return backingStore_->compareObjectsById(slOid, filteredOid.object());
    } else {
      // We know we have a filtered tree and an unfiltered underlying id
      // (presumably also a tree). They must be different since one is filtered.
      return ObjectComparison::Different;
    }
  }

  // We must interpret the ObjectIds as FilteredIds (FOIDs) so we can access
  // the components of the FOIDs.
  FilteredObjectId filteredOne = FilteredObjectId::fromObjectId(one);
  auto typeOne = filteredOne.objectType();
  FilteredObjectId filteredTwo = FilteredObjectId::fromObjectId(two);
  auto typeTwo = filteredTwo.objectType();

  // We're comparing ObjectIDs of different types. The objects are not equal.
  if (typeOne != typeTwo) {
    XLOGF(
        DBG9,
        "Attempted to compare: {} vs {} (types: {} vs {})",
        one.toLogString(),
        two.toLogString(),
        foidTypeToString(typeOne),
        foidTypeToString(typeTwo));
    return ObjectComparison::Different;
  }

  // ======= Blob Object Handling =======

  // When comparing blob objects, we only need to check if the underlying
  // ObjectIds resolve to equal.
  if (typeOne == FilteredObjectIdType::OBJECT_TYPE_BLOB) {
    return backingStore_->compareObjectsById(
        filteredOne.object(), filteredTwo.object());
  }

  // ======= Unfiltered Tree Object Handling =======

  // We're comparing two recursively unfiltered trees. We can fall back to
  // the underlying BackingStore's comparison logic.
  if (typeOne == typeTwo &&
      typeOne == FilteredObjectIdType::OBJECT_TYPE_UNFILTERED_TREE) {
    return backingStore_->compareObjectsById(
        filteredOne.object(), filteredTwo.object());
  }

  // When comparing tree objects, we need to consider filter changes.
  if (typeOne == FilteredObjectIdType::OBJECT_TYPE_TREE ||
      typeTwo == FilteredObjectIdType::OBJECT_TYPE_TREE) {
    // If the filters are the same, then we can simply check whether the
    // underlying ObjectIds resolve to equal.
    if (filter_->areFiltersIdentical(
            filteredOne.filter(), filteredTwo.filter())) {
      return backingStore_->compareObjectsById(
          filteredOne.object(), filteredTwo.object());
    }

    // If the filters are different, we need to resolve whether the filter
    // change affected the underlying object. This is difficult to do, and
    // is infeasible with the current FilteredBackingStore implementation.
    // Instead, we will return Unknown for any filter changes that we are
    // unsure about.
    auto pathAffected = pathAffectedByFilterChange(
        filteredOne.path(),
        filteredTwo.path(),
        filteredOne.filter(),
        filteredTwo.filter());
    if (pathAffected.isReady()) {
      auto filterComparison = std::move(pathAffected).get();

      // If the filters are identical, we need to check whether the underlying
      // Objects are identical. In other words, the filters being identical is
      // not enough to confirm that the objects are identical.
      if (filterComparison == ObjectComparison::Identical) {
        return backingStore_->compareObjectsById(
            filteredOne.object(), filteredTwo.object());
      } else {
        // If the filter coverage is different, the objects must be filtered
        // differently (or we can't confirm they're filtered the same way).
        return filterComparison;
      }
    } else {
      // We can't immediately tell if the path is affected by the filter
      // change. Instead of chaining the future and queueing up a bunch of
      // work, we'll return Unknown early.
      return ObjectComparison::Unknown;
    }
  } else {
    // We received something other than a tree, blob, or filtered tree. Throw.
    throwf<std::runtime_error>(
        "Unknown object type: {}", foidTypeToString(typeOne));
  }
}

ObjectComparison FilteredBackingStore::compareRootsById(
    const RootId& one,
    const RootId& two) {
  // Fast path: bytewise equal must also be semantically equal
  if (one == two) {
    return ObjectComparison::Identical;
  }

  // FilteredRootIds are composed of two parts: the underlying RootId and the
  // filter ID. Both must be semantically equal to be considered identical.
  auto [underlyingRootIdOne, filterIdOne] = parseFilterIdFromRootId(one);
  auto [underlyingRootIdTwo, filterIdTwo] = parseFilterIdFromRootId(two);

  auto underlyingComparison =
      backingStore_->compareRootsById(underlyingRootIdOne, underlyingRootIdTwo);
  if (underlyingComparison == ObjectComparison::Different) {
    return ObjectComparison::Different;
  }

  // FilterId -> Filter mappings are not bijective, so we need to check
  // semantic equality instead of bytewise equality if bytewise comparison
  // fails.
  bool filtersIdentical = (filterIdOne == filterIdTwo) ||
      filter_->areFiltersIdentical(filterIdOne, filterIdTwo);

  if (filtersIdentical) {
    // Both the underlying RootIds and filters are identical, so return the
    // underlying comparison result (which could be Identical or Unknown).
    return underlyingComparison;
  }

  // The filters are different. Even if the underlying RootIds are identical,
  // different filters mean different effective roots.
  return ObjectComparison::Different;
}

ImmediateFuture<std::unique_ptr<PathMap<TreeEntry>>>
FilteredBackingStore::filterImpl(
    const TreePtr unfilteredTree,
    RelativePathPiece treePath,
    folly::StringPiece filterId,
    FilteredObjectIdType treeType) {
  // DEPRECATED: use co_filterImpl directly. Kept only because getRootTree and
  // getTree (future versions) still call it; delete once BackingStore interface
  // drops the future-based getTree/getRootTree virtuals.
  return ImmediateFuture{
      // @lint-ignore CLANGTIDY facebook-folly-coro-return-captures-local-var
      folly::coro::co_invoke(
          [this](auto&&... args)
              -> folly::coro::Task<std::unique_ptr<PathMap<TreeEntry>>> {
            co_return co_await co_filterImpl(
                std::forward<decltype(args)>(args)...);
          },
          unfilteredTree,
          treePath.copy(),
          filterId.toString(),
          treeType)
          .semi()};
}

folly::coro::now_task<std::unique_ptr<PathMap<TreeEntry>>>
FilteredBackingStore::co_filterImpl(
    const TreePtr unfilteredTree,
    RelativePathPiece treePath,
    folly::StringPiece filterId,
    FilteredObjectIdType treeType) {
  // Determine whether each child should be filtered. Failure to determine
  // whether a file is filtered would cause it to disappear from the source
  // tree. Instead of leaving users in a weird state where some files are
  // missing, we let the exception propagate to fail the entire getTree()
  // request so the caller can decide to retry.
  //
  // collectAllRange matches the futures path's collectAllSafe — all checks
  // run concurrently and any failure aborts the entire request.
  std::vector<folly::coro::Task<std::pair<RelativePath, FilterCoverage>>>
      filterTasks;
  for (const auto& [path, entry] : *unfilteredTree) {
    auto relPath = RelativePath{treePath + path};

    if (treeType == FilteredObjectIdType::OBJECT_TYPE_TREE) {
      filterTasks.emplace_back(
          folly::coro::co_invoke(
              [](Filter* filter, RelativePath rp, std::string fid)
                  -> folly::coro::Task<
                      std::pair<RelativePath, FilterCoverage>> {
                auto coverage =
                    co_await filter->co_getFilterCoverageForPath(rp, fid);
                co_return std::pair(std::move(rp), coverage);
              },
              filter_.get(),
              std::move(relPath),
              std::string{filterId}));
    } else if (treeType == FilteredObjectIdType::OBJECT_TYPE_UNFILTERED_TREE) {
      filterTasks.emplace_back(
          folly::coro::co_invoke(
              [](RelativePath rp) -> folly::coro::Task<
                                      std::pair<RelativePath, FilterCoverage>> {
                co_return std::pair(
                    std::move(rp), FilterCoverage::RECURSIVELY_UNFILTERED);
              },
              std::move(relPath)));
    } else {
      throwf<std::invalid_argument>(
          "FilterImpl() received an unexpected tree type: {}",
          foidTypeToString(treeType));
    }
  }

  auto filterResults =
      co_await folly::coro::collectAllRange(std::move(filterTasks));

  // Build PathMap from filter results
  auto pathMap = PathMap<TreeEntry>{unfilteredTree->getCaseSensitivity()};
  for (auto& [relPath, filterCoverage] : filterResults) {
    if (filterCoverage == FilterCoverage::RECURSIVELY_FILTERED) {
      continue;
    }

    auto entry = unfilteredTree->find(relPath.basename().piece());
    auto entryType = entry->second.getType();
    ObjectId oid;

    if (entryType == TreeEntryType::TREE) {
      if (filterCoverage == FilterCoverage::UNFILTERED) {
        auto foid = FilteredObjectId(
            relPath.piece(), filterId, entry->second.getObjectId());
        oid = ObjectId{foid.getValue()};
      } else {
        auto foid = FilteredObjectId{
            entry->second.getObjectId(),
            FilteredObjectIdType::OBJECT_TYPE_UNFILTERED_TREE};
        oid = ObjectId{foid.getValue()};
      }
    } else {
      auto foid = FilteredObjectId{
          entry->second.getObjectId(), FilteredObjectIdType::OBJECT_TYPE_BLOB};
      oid = ObjectId{foid.getValue()};
    }

    auto treeEntry = TreeEntry{
        std::move(oid),
        entryType,
        entry->second.getSize(),
        entry->second.getContentSha1(),
        entry->second.getContentBlake3(),
        entry->second.isRestricted(),
        entry->second.hasACL()};
    pathMap.insert(std::pair{relPath.basename().copy(), std::move(treeEntry)});
  }

  // The result is a PathMap containing only unfiltered or
  // recursively-unfiltered tree entries.
  co_return std::make_unique<PathMap<TreeEntry>>(std::move(pathMap));
}

ImmediateFuture<BackingStore::GetRootTreeResult>
FilteredBackingStore::getRootTree(
    const RootId& rootId,
    const ObjectFetchContextPtr& context) {
  return ImmediateFuture{
      // @lint-ignore CLANGTIDY facebook-folly-coro-return-captures-local-var
      folly::coro::co_withExecutor(
          folly::getGlobalCPUExecutor(),
          folly::coro::co_invoke(
              [self = shared_from_this()](auto rootId, auto context)
                  -> folly::coro::Task<GetRootTreeResult> {
                co_return co_await self->co_getRootTree(
                    std::move(rootId), std::move(context));
              },
              RootId{rootId},
              context.copy()))
          .start()};
}

folly::coro::now_task<BackingStore::GetRootTreeResult>
FilteredBackingStore::co_getRootTree(
    const RootId& rootId,
    const ObjectFetchContextPtr& context) {
  auto [parsedRootId, filterId] = parseFilterIdFromRootId(rootId);
  XLOGF(
      DBG7, "co_getRootTree {} with filter {}", parsedRootId.value(), filterId);
  auto rootTreeResult =
      co_await backingStore_->co_getRootTree(parsedRootId, context);

  // Apply the filter to the root tree. The root tree is always a regular
  // "unfiltered" tree.
  auto pathMap = co_await co_filterImpl(
      rootTreeResult.tree,
      RelativePath{""},
      filterId,
      FilteredObjectIdType::OBJECT_TYPE_TREE);

  auto rootFOID =
      FilteredObjectId{RelativePath{""}, filterId, rootTreeResult.treeId};
  co_return GetRootTreeResult{
      rootTreeResult.tree->withNewId(
          std::move(*pathMap), ObjectId{rootFOID.getValue()}),
      ObjectId{rootFOID.getValue()},
  };
}

ImmediateFuture<std::shared_ptr<TreeEntry>>
FilteredBackingStore::getTreeEntryForObjectId(
    const ObjectId& objectId,
    TreeEntryType treeEntryType,
    const ObjectFetchContextPtr& context) {
  if (isSlOid(objectId)) {
    // Raw id from underlying backingstore, meaning unfiltered fast path.
    return backingStore_->getTreeEntryForObjectId(
        objectId, treeEntryType, context);
  }

  FilteredObjectId filteredId = FilteredObjectId::fromObjectId(objectId);
  return backingStore_->getTreeEntryForObjectId(
      filteredId.object(), treeEntryType, context);
}

folly::SemiFuture<BackingStore::GetTreeAuxResult>
FilteredBackingStore::getTreeAuxData(
    const ObjectId& id,
    const ObjectFetchContextPtr& context) {
  if (isSlOid(id)) {
    // Raw id from underlying backingstore, meaning unfiltered fast path.
    return backingStore_->getTreeAuxData(id, context);
  }

  // TODO(cuev): This is wrong. This is only correct for the case where the
  // user doesn't care about the filter-ness of the tree. We should figure out
  // what the optimal behavior of this function is (i.e. if it should respect
  // filters or not).
  auto filteredId = FilteredObjectId::fromObjectId(id);
  return backingStore_->getTreeAuxData(filteredId.object(), context);
}

folly::SemiFuture<BackingStore::GetTreeResult> FilteredBackingStore::getTree(
    const ObjectId& id,
    const ObjectFetchContextPtr& context) {
  if (isSlOid(id)) {
    // Raw id from underlying backingstore, meaning unfiltered fast path.
    return backingStore_->getTree(id, context);
  }

  auto filteredId = FilteredObjectId::fromObjectId(id);
  auto unfilteredTree = backingStore_->getTree(filteredId.object(), context);
  return std::move(unfilteredTree)
      .deferValue([self = shared_from_this(),
                   filteredId = std::move(filteredId)](GetTreeResult&& result) {
        auto treeType = filteredId.objectType();
        if (treeType == FilteredObjectIdType::OBJECT_TYPE_UNFILTERED_TREE &&
            self->isSaplingBackingStore_ && self->optimizeUnfilteredTrees_) {
          // Tree is recursively unfiltered - activate fast path by not
          // rewriting ids within the tree entries. We still copy the tree so we
          // can modify its oid to match the requested oid.
          result.tree = result.tree->withNewId(ObjectId{filteredId.getValue()});
          return ImmediateFuture<GetTreeResult>{std::move(result)}.semi();
        }

        auto filterRes = treeType == FilteredObjectIdType::OBJECT_TYPE_TREE
            ? self->filterImpl(
                  result.tree, filteredId.path(), filteredId.filter(), treeType)
            : self->filterImpl(result.tree, RelativePath{}, "", treeType);
        return std::move(filterRes)
            .thenValue([filteredId,
                        origin = result.origin,
                        sourceTree = std::move(result.tree)](
                           std::unique_ptr<PathMap<TreeEntry>> pathMap) {
              auto tree = sourceTree->withNewId(
                  std::move(*pathMap), ObjectId{filteredId.getValue()});
              pathMap.reset();
              return GetTreeResult{std::move(tree), origin};
            })
            .semi();
      });
}

folly::coro::now_task<BackingStore::GetTreeResult>
FilteredBackingStore::co_getTree(
    const ObjectId& id,
    const ObjectFetchContextPtr& context) {
  if (isSlOid(id)) {
    // Raw id from underlying backingstore, meaning unfiltered fast path.
    co_return co_await backingStore_->co_getTree(id, context);
  }

  auto filteredId = FilteredObjectId::fromObjectId(id);
  auto result =
      co_await backingStore_->co_getTree(filteredId.object(), context);

  auto treeType = filteredId.objectType();
  if (treeType == FilteredObjectIdType::OBJECT_TYPE_UNFILTERED_TREE &&
      isSaplingBackingStore_ && optimizeUnfilteredTrees_) {
    // Tree is recursively unfiltered - activate fast path by not
    // rewriting ids within the tree entries. We still copy the tree so we
    // can modify its oid to match the requested oid.
    result.tree = result.tree->withNewId(ObjectId{filteredId.getValue()});
    co_return result;
  }

  auto pathMap = treeType == FilteredObjectIdType::OBJECT_TYPE_TREE
      ? co_await co_filterImpl(
            result.tree, filteredId.path(), filteredId.filter(), treeType)
      : co_await co_filterImpl(result.tree, RelativePath{}, "", treeType);
  auto tree = result.tree->withNewId(
      std::move(*pathMap), ObjectId{filteredId.getValue()});
  pathMap.reset();
  co_return GetTreeResult{std::move(tree), result.origin};
}

folly::coro::now_task<BackingStore::GetTreeAuxResult>
FilteredBackingStore::co_getTreeAuxData(
    const ObjectId& id,
    const ObjectFetchContextPtr& context) {
  if (isSlOid(id)) {
    co_return co_await backingStore_->co_getTreeAuxData(id, context);
  }
  // TODO(cuev): This is wrong. This is only correct for the case where the
  // user doesn't care about the filter-ness of the tree. We should figure out
  // what the optimal behavior of this function is (i.e. if it should respect
  // filters or not).
  auto filteredId = FilteredObjectId::fromObjectId(id);
  co_return co_await backingStore_->co_getTreeAuxData(
      filteredId.object(), context);
}

folly::SemiFuture<BackingStore::GetBlobAuxResult>
FilteredBackingStore::getBlobAuxData(
    const ObjectId& id,
    const ObjectFetchContextPtr& context) {
  if (isSlOid(id)) {
    // Raw id from underlying backingstore, meaning unfiltered fast path.
    return backingStore_->getBlobAuxData(id, context);
  }

  auto filteredId = FilteredObjectId::fromObjectId(id);
  return backingStore_->getBlobAuxData(filteredId.object(), context);
}

folly::coro::now_task<BackingStore::GetBlobAuxResult>
FilteredBackingStore::co_getBlobAuxData(
    const ObjectId& id,
    const ObjectFetchContextPtr& context) {
  if (isSlOid(id)) {
    co_return co_await backingStore_->co_getBlobAuxData(id, context);
  }

  auto filteredId = FilteredObjectId::fromObjectId(id);
  co_return co_await backingStore_->co_getBlobAuxData(
      filteredId.object(), context);
}

folly::SemiFuture<BackingStore::GetBlobResult> FilteredBackingStore::getBlob(
    const ObjectId& id,
    const ObjectFetchContextPtr& context) {
  if (isSlOid(id)) {
    // Raw id from underlying backingstore, meaning unfiltered fast path.
    return backingStore_->getBlob(id, context);
  }

  auto filteredId = FilteredObjectId::fromObjectId(id);
  return backingStore_->getBlob(filteredId.object(), context);
}

folly::coro::Task<BackingStore::GetBlobResult> FilteredBackingStore::co_getBlob(
    const ObjectId& id,
    const ObjectFetchContextPtr& context) {
  if (isSlOid(id)) {
    // Raw id from underlying backingstore, meaning unfiltered fast path.
    co_return co_await backingStore_->co_getBlob(id, context);
  }

  auto filteredId = FilteredObjectId::fromObjectId(id);
  co_return co_await backingStore_->co_getBlob(filteredId.object(), context);
}

ImmediateFuture<bool> FilteredBackingStore::checkPermission(
    const ObjectId& manifestId) {
  if (isSlOid(manifestId)) {
    return backingStore_->checkPermission(manifestId);
  }

  return backingStore_->checkPermission(
      FilteredObjectId::fromObjectId(manifestId).object());
}

folly::coro::now_task<std::vector<folly::Try<std::vector<EntryAcl>>>>
FilteredBackingStore::co_getPathAcls(
    const RootId& rootId,
    const std::vector<std::string>& paths,
    const ObjectFetchContextPtr& context) {
  auto [parsedRootId, _] = parseFilterIdFromRootId(rootId);
  co_return co_await backingStore_->co_getPathAcls(
      parsedRootId, paths, context);
}

folly::coro::now_task<folly::Unit> FilteredBackingStore::co_prefetchBlobs(
    ObjectIdRange ids,
    const ObjectFetchContextPtr& context) {
  // Fast path: avoid allocation if all ids can be passed through.
  if (std::all_of(
          ids.begin(), ids.end(), [this](auto& id) { return isSlOid(id); })) {
    co_await backingStore_->co_prefetchBlobs(ids, context);
    co_return folly::unit;
  }

  std::vector<ObjectId> unfilteredIds;
  unfilteredIds.reserve(ids.size());
  std::transform(
      ids.begin(),
      ids.end(),
      std::back_inserter(unfilteredIds),
      [this](auto& id) {
        if (isSlOid(id)) {
          return id;
        }
        return FilteredObjectId::fromObjectId(id).object();
      });
  co_await backingStore_->co_prefetchBlobs(unfilteredIds, context);
  co_return folly::unit;
}

ImmediateFuture<BackingStore::GetGlobFilesResult>
FilteredBackingStore::getGlobFiles(
    const RootId& id,
    const std::vector<std::string>& globs,
    const std::vector<std::string>& prefixes) {
  auto [parsedRootId, parsedFilterId] = parseFilterIdFromRootId(id);
  auto fut = backingStore_->getGlobFiles(parsedRootId, globs, prefixes);
  return std::move(fut).thenValue([self = shared_from_this(),
                                   id,
                                   filterId = parsedFilterId](
                                      auto&& getGlobFilesResult) {
    std::vector<ImmediateFuture<std::pair<std::string, FilterCoverage>>>
        isFilteredFutures;
    isFilteredFutures.reserve(getGlobFilesResult.globFiles.size());
    for (std::string& path : getGlobFilesResult.globFiles) {
      auto filterResult = self->filter_->getFilterCoverageForPath(
          RelativePathPiece(path), filterId);
      auto filterFut =
          std::move(filterResult)
              .thenValue([path = std::move(path)](auto&& coverage) mutable {
                return std::pair(std::move(path), std::move(coverage));
              });
      isFilteredFutures.emplace_back(std::move(filterFut));
    }
    return collectAllSafe(std::move(isFilteredFutures))
        .thenValue([rootId = id](
                       std::vector<std::pair<std::string, FilterCoverage>>&&
                           filterCoverageVec) {
          std::vector<std::string> filteredPaths;
          for (auto&& filterCoveragePair : filterCoverageVec) {
            auto filterCoverage = filterCoveragePair.second;
            // Let through unfiltered paths
            if (filterCoverage != FilterCoverage::RECURSIVELY_FILTERED) {
              filteredPaths.emplace_back(std::move(filterCoveragePair.first));
            }
            // If the filterCoverage is RECURSIVELY_FILTERED, just drop it
          }
          return GetGlobFilesResult{filteredPaths, std::move(rootId)};
        });
  });
}

folly::coro::now_task<BackingStore::GetGlobFilesResult>
FilteredBackingStore::co_getGlobFiles(
    const RootId& id,
    const std::vector<std::string>& globs,
    const std::vector<std::string>& prefixes) {
  auto [parsedRootId, parsedFilterId] = parseFilterIdFromRootId(id);
  auto getGlobFilesResult =
      co_await backingStore_->co_getGlobFiles(parsedRootId, globs, prefixes);

  // Parallelize filter checks matching the futures path's collectAllSafe.
  std::vector<folly::coro::Task<std::pair<std::string, FilterCoverage>>>
      filterTasks;
  filterTasks.reserve(getGlobFilesResult.globFiles.size());
  for (auto& path : getGlobFilesResult.globFiles) {
    filterTasks.emplace_back(
        folly::coro::co_invoke(
            [](Filter* filter, std::string p, std::string fid)
                -> folly::coro::Task<std::pair<std::string, FilterCoverage>> {
              auto coverage = co_await filter->co_getFilterCoverageForPath(
                  RelativePathPiece(p), fid);
              co_return std::pair(std::move(p), coverage);
            },
            filter_.get(),
            std::move(path),
            std::string{parsedFilterId}));
  }
  auto filterResults =
      co_await folly::coro::collectAllRange(std::move(filterTasks));

  std::vector<std::string> filteredPaths;
  for (auto& [path, coverage] : filterResults) {
    if (coverage != FilterCoverage::RECURSIVELY_FILTERED) {
      filteredPaths.emplace_back(std::move(path));
    }
  }
  co_return GetGlobFilesResult{std::move(filteredPaths), id};
}

void FilteredBackingStore::periodicManagementTask() {
  backingStore_->periodicManagementTask();
}

void FilteredBackingStore::startRecordingFetch() {
  backingStore_->startRecordingFetch();
}

std::unordered_set<std::string> FilteredBackingStore::stopRecordingFetch() {
  return backingStore_->stopRecordingFetch();
}

ImmediateFuture<folly::Unit> FilteredBackingStore::importManifestForRoot(
    const RootId& rootId,
    const Hash20& manifest,
    const ObjectFetchContextPtr& context) {
  // The manifest passed to this function will be unfiltered (i.e. it won't
  // be a FilteredRootId or FilteredObjectId), so we pass it directly to the
  // underlying BackingStore.
  auto [parsedRootId, _] = parseFilterIdFromRootId(rootId);
  return backingStore_->importManifestForRoot(parsedRootId, manifest, context);
}

folly::coro::now_task<folly::Unit>
FilteredBackingStore::co_importManifestForRoot(
    const RootId& rootId,
    const Hash20& manifest,
    const ObjectFetchContextPtr& context) {
  auto [parsedRootId, _] = parseFilterIdFromRootId(rootId);
  co_return co_await backingStore_->co_importManifestForRoot(
      parsedRootId, manifest, context);
}

RootId FilteredBackingStore::parseRootId(folly::StringPiece rootId) {
  auto [startingRootId, filterId] =
      parseFilterIdFromRootId(RootId{rootId.toString()});
  auto parsedRootId = backingStore_->parseRootId(startingRootId.value());
  XLOGF(
      DBG7, "Parsed RootId {} with filter {}", parsedRootId.value(), filterId);
  return RootId{createFilteredRootId(
      std::move(parsedRootId).value(), std::move(filterId))};
}

void FilteredBackingStore::workingCopyParentHint(const RootId& parent) {
  // Pass along the root id sans filter id.
  auto [startingRootId, _] = parseFilterIdFromRootId(parent);
  backingStore_->workingCopyParentHint(startingRootId);
}

std::string FilteredBackingStore::renderRootId(const RootId& rootId) {
  auto [underlyingRootId, _] = parseFilterIdFromRootId(rootId);
  return backingStore_->renderRootId(underlyingRootId);
}

std::string FilteredBackingStore::displayRootId(const RootId& rootId) {
  auto [underlyingRootId, filterId] = parseFilterIdFromRootId(rootId);
  return fmt::format(
      "{} fid={}",
      backingStore_->displayRootId(underlyingRootId),
      folly::hexlify(filterId));
}

ObjectId FilteredBackingStore::parseObjectId(folly::StringPiece objectId) {
  if (!FilteredObjectId::isFilteredObjectIdString(objectId)) {
    return backingStore_->parseObjectId(objectId);
  }
  auto foid = FilteredObjectId::parseFilteredObjectId(objectId, backingStore_);
  return ObjectId{foid.getValue()};
}

std::string FilteredBackingStore::renderObjectId(const ObjectId& id) {
  XLOGF(DBG8, "Rendering FilteredObjectId: {}", id.asString());
  if (isSlOid(id)) {
    // Raw id from underlying backingstore.
    return backingStore_->renderObjectId(id);
  }

  auto filteredId = FilteredObjectId::fromObjectId(id);
  auto object = filteredId.object();
  auto underlyingOid = backingStore_->renderObjectId(object);
  return FilteredObjectId::renderFilteredObjectId(filteredId, underlyingOid);
}

std::optional<folly::StringPiece> FilteredBackingStore::getRepoName() {
  return backingStore_->getRepoName();
}

std::string FilteredBackingStore::createFilteredRootId(
    std::string_view originalRootId,
    std::string_view filterId) {
  size_t originalRootIdSize = originalRootId.size();
  uint8_t varintBuf[folly::kMaxVarintLength64] = {};
  size_t encodedSize = folly::encodeVarint(originalRootIdSize, varintBuf);
  std::string buf;
  buf.reserve(encodedSize + originalRootIdSize + filterId.size());
  buf.append(reinterpret_cast<const char*>(varintBuf), encodedSize);
  buf.append(originalRootId);
  buf.append(filterId);
  XLOGF(
      DBG7,
      "Created FilteredRootId: {} from Original Root Size: {}, Original RootId: {}, FilterID: {}",
      buf,
      originalRootIdSize,
      originalRootId,
      filterId);
  return buf;
}
} // namespace facebook::eden
