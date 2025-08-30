/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/FilteredBackingStore.h"

#include <folly/Varint.h>
#include <stdexcept>
#include <tuple>

#include "eden/common/utils/ImmediateFuture.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/ObjectId.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/BackingStore.h"
#include "eden/fs/store/filter/Filter.h"
#include "eden/fs/store/filter/FilteredObjectId.h"
#include "eden/fs/utils/FilterUtils.h"

namespace facebook::eden {

FilteredBackingStore::FilteredBackingStore(
    std::shared_ptr<BackingStore> backingStore,
    std::unique_ptr<Filter> filter)
    : backingStore_{std::move(backingStore)}, filter_{std::move(filter)} {}

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

ObjectComparison FilteredBackingStore::compareObjectsById(
    const ObjectId& one,
    const ObjectId& two) {
  // If the two objects have the same bytes, then they are using the same
  // filter and must be equal.
  if (one == two) {
    return ObjectComparison::Identical;
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
        DBG2,
        "Attempted to compare: {} vs {} (types: {} vs {})",
        filteredOne.getValue(),
        filteredTwo.getValue(),
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
    if (filteredOne.filter() == filteredTwo.filter()) {
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

ImmediateFuture<std::unique_ptr<PathMap<TreeEntry>>>
FilteredBackingStore::filterImpl(
    const TreePtr unfilteredTree,
    RelativePathPiece treePath,
    folly::StringPiece filterId,
    FilteredObjectIdType treeType) {
  // First we determine whether each child should be filtered.
  auto isFilteredFutures =
      std::vector<ImmediateFuture<std::pair<RelativePath, FilterCoverage>>>{};

  // The FilterID is passed through multiple futures. Let's create a copy and
  // pass it around to avoid lifetime issues.
  auto filter = filterId.toString();
  for (const auto& [path, entry] : *unfilteredTree) {
    auto relPath = RelativePath{treePath + path};

    // For normal (unfiltered) trees, we call into Mercurial to determine
    // whether each child is filtered or not.
    if (treeType == FilteredObjectIdType::OBJECT_TYPE_TREE) {
      auto filteredRes = filter_->getFilterCoverageForPath(relPath, filter);
      auto fut = std::move(filteredRes)
                     .thenValue([relPath = std::move(relPath)](
                                    FilterCoverage coverage) mutable {
                       return std::pair(std::move(relPath), coverage);
                     });
      isFilteredFutures.emplace_back(std::move(fut));
    } else if (treeType == FilteredObjectIdType::OBJECT_TYPE_UNFILTERED_TREE) {
      // For recursively unfiltered trees, we know that every child will also be
      // recursively unfiltered. Therefore, we can avoid the cost of calling
      // into Mercurial to check each child.
      isFilteredFutures.emplace_back(
          ImmediateFuture<std::pair<RelativePath, FilterCoverage>>{
              {std::move(relPath), FilterCoverage::RECURSIVELY_UNFILTERED}});
    } else {
      // OBJECT_TYPE_BLOB should never be passed to filterImpl
      throwf<std::invalid_argument>(
          "FilterImpl() received an unexpected tree type: {}",
          foidTypeToString(treeType));
    }
  }

  // CollectAllSafe is intentional -- failure to determine whether a file is
  // filtered would cause it to disappear from the source tree. Instead of
  // leaving users in a weird state where some files are missing, we'll fail
  // the entire getTree() request and the caller can decide to retry.
  return collectAllSafe(std::move(isFilteredFutures))
      .thenValue(
          [unfilteredTree, filterId = std::move(filter)](
              std::vector<std::pair<RelativePath, FilterCoverage>>&&
                  filterCoverageVec) -> std::unique_ptr<PathMap<TreeEntry>> {
            // This PathMap will only contain tree entries that aren't
            // filtered
            auto pathMap =
                PathMap<TreeEntry>{unfilteredTree->getCaseSensitivity()};

            for (auto&& filterCoveragePair : filterCoverageVec) {
              auto filterCoverage = filterCoveragePair.second;

              // We need to re-add unfiltered entries to the path map.
              if (filterCoverage != FilterCoverage::RECURSIVELY_FILTERED) {
                auto relPath = std::move(filterCoveragePair.first);
                auto entry = unfilteredTree->find(relPath.basename().piece());
                auto entryType = entry->second.getType();
                ObjectId oid;

                // The entry type is a tree. Trees can either be unfiltered or
                // recursively unfiltered. We handle these cases differently.
                if (entryType == TreeEntryType::TREE) {
                  if (filterCoverage == FilterCoverage::UNFILTERED) {
                    // We can't guarantee all the trees descendents are
                    // filtered, so we need to create a normal tree FOID
                    auto foid = FilteredObjectId(
                        relPath.piece(), filterId, entry->second.getObjectId());
                    oid = ObjectId{foid.getValue()};
                  } else {
                    // We can guarantee that all the descendents of this tree
                    // are unfiltered. We can special case this tree to avoid
                    // recursive filter lookups in the future.
                    auto foid = FilteredObjectId{
                        entry->second.getObjectId(),
                        FilteredObjectIdType::OBJECT_TYPE_UNFILTERED_TREE};
                    oid = ObjectId{foid.getValue()};
                  }
                } else {
                  // Blobs are the same regardless of recursive/non-recursive
                  // FilterCoverage.
                  auto foid = FilteredObjectId{
                      entry->second.getObjectId(),
                      FilteredObjectIdType::OBJECT_TYPE_BLOB};
                  oid = ObjectId{foid.getValue()};
                }

                // Regardless of FilteredObjectIdType, all unfiltered entries
                // need to be placed into the unfiltered PathMap.
                auto treeEntry = TreeEntry{std::move(oid), entryType};
                auto pair =
                    std::pair{relPath.basename().copy(), std::move(treeEntry)};
                pathMap.insert(std::move(pair));
              }
              // Recursively filtered objects don't need to be handled. They are
              // simply omitted from the PathMap.
            }

            // The result is a PathMap containing only unfiltered or
            // recursively-unfiltered tree entries.
            return std::make_unique<PathMap<TreeEntry>>(std::move(pathMap));
          });
}

ImmediateFuture<BackingStore::GetRootTreeResult>
FilteredBackingStore::getRootTree(
    const RootId& rootId,
    const ObjectFetchContextPtr& context) {
  auto [parsedRootId, filterId] = parseFilterIdFromRootId(rootId);
  XLOGF(
      DBG7,
      "Getting rootTree {} with filter {}",
      parsedRootId.value(),
      filterId);
  auto fut = backingStore_->getRootTree(parsedRootId, context);
  return std::move(fut).thenValue(
      [filterId_2 = std::move(filterId),
       self = shared_from_this()](GetRootTreeResult rootTreeResult) mutable {
        // Apply the filter to the root tree. The root tree is always a regular
        // "unfiltered" tree.
        auto filterFut = self->filterImpl(
            rootTreeResult.tree,
            RelativePath{""},
            filterId_2,
            FilteredObjectIdType::OBJECT_TYPE_TREE);
        return std::move(filterFut).thenValue(
            [self,
             filterId_3 = std::move(filterId_2),
             treeId = std::move(rootTreeResult.treeId)](
                std::unique_ptr<PathMap<TreeEntry>> pathMap) {
              auto rootFOID =
                  FilteredObjectId{RelativePath{""}, filterId_3, treeId};
              auto res = GetRootTreeResult{
                  std::make_shared<const Tree>(
                      std::move(*pathMap), ObjectId{rootFOID.getValue()}),
                  ObjectId{rootFOID.getValue()},
              };
              pathMap.reset();
              return res;
            });
      });
}

ImmediateFuture<std::shared_ptr<TreeEntry>>
FilteredBackingStore::getTreeEntryForObjectId(
    const ObjectId& objectId,
    TreeEntryType treeEntryType,
    const ObjectFetchContextPtr& context) {
  FilteredObjectId filteredId = FilteredObjectId::fromObjectId(objectId);
  return backingStore_->getTreeEntryForObjectId(
      filteredId.object(), treeEntryType, context);
}

folly::SemiFuture<BackingStore::GetTreeAuxResult>
FilteredBackingStore::getTreeAuxData(
    const ObjectId& id,
    const ObjectFetchContextPtr& context) {
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
  auto filteredId = FilteredObjectId::fromObjectId(id);
  auto unfilteredTree = backingStore_->getTree(filteredId.object(), context);
  return std::move(unfilteredTree)
      .deferValue([self = shared_from_this(),
                   filteredId = std::move(filteredId)](GetTreeResult&& result) {
        auto treeType = filteredId.objectType();
        auto filterRes = treeType == FilteredObjectIdType::OBJECT_TYPE_TREE
            ? self->filterImpl(
                  result.tree, filteredId.path(), filteredId.filter(), treeType)
            : self->filterImpl(result.tree, RelativePath{}, "", treeType);
        return std::move(filterRes)
            .thenValue([filteredId, origin = result.origin](
                           std::unique_ptr<PathMap<TreeEntry>> pathMap) {
              auto tree = std::make_shared<Tree>(
                  std::move(*pathMap), ObjectId{filteredId.getValue()});
              pathMap.reset();
              return GetTreeResult{std::move(tree), origin};
            })
            .semi();
      });
}

folly::SemiFuture<BackingStore::GetBlobAuxResult>
FilteredBackingStore::getBlobAuxData(
    const ObjectId& id,
    const ObjectFetchContextPtr& context) {
  auto filteredId = FilteredObjectId::fromObjectId(id);
  return backingStore_->getBlobAuxData(filteredId.object(), context);
}

folly::SemiFuture<BackingStore::GetBlobResult> FilteredBackingStore::getBlob(
    const ObjectId& id,
    const ObjectFetchContextPtr& context) {
  auto filteredId = FilteredObjectId::fromObjectId(id);
  return backingStore_->getBlob(filteredId.object(), context);
}

folly::SemiFuture<folly::Unit> FilteredBackingStore::prefetchBlobs(
    ObjectIdRange ids,
    const ObjectFetchContextPtr& context) {
  std::vector<ObjectId> unfilteredIds;
  unfilteredIds.reserve(ids.size());
  std::transform(
      ids.begin(), ids.end(), std::back_inserter(unfilteredIds), [](auto& id) {
        return FilteredObjectId::fromObjectId(id).object();
      });
  // prefetchBlobs() expects that the caller guarantees the ids live at least
  // longer than this future takes to complete. Therefore, we ensure the
  // lifetime of the newlly created unfilteredIds.
  auto fut = backingStore_->prefetchBlobs(unfilteredIds, context);
  return std::move(fut).deferEnsure(
      [unfilteredIds = std::move(unfilteredIds)]() {});
}

ImmediateFuture<BackingStore::GetGlobFilesResult>
FilteredBackingStore::getGlobFiles(
    const RootId& id,
    const std::vector<std::string>& globs,
    const std::vector<std::string>& prefixes) {
  auto [parsedRootId, parsedFilterId] = parseFilterIdFromRootId(id);
  auto fut = backingStore_->getGlobFiles(parsedRootId, globs, prefixes);
  return std::move(fut).thenValue([this, id, filterId = parsedFilterId](
                                      auto&& getGlobFilesResult) {
    std::vector<ImmediateFuture<std::pair<std::string, FilterCoverage>>>
        isFilteredFutures;
    isFilteredFutures.reserve(getGlobFilesResult.globFiles.size());
    for (std::string& path : getGlobFilesResult.globFiles) {
      auto filterResult =
          filter_->getFilterCoverageForPath(RelativePathPiece(path), filterId);
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

ObjectId FilteredBackingStore::parseObjectId(folly::StringPiece objectId) {
  auto foid = FilteredObjectId::parseFilteredObjectId(objectId, backingStore_);
  return ObjectId{foid.getValue()};
}

std::string FilteredBackingStore::renderObjectId(const ObjectId& id) {
  XLOGF(DBG8, "Rendering FilteredObjectId: {}", id.asString());
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
