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
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/filter/Filter.h"
#include "eden/fs/store/filter/FilteredObjectId.h"
#include "eden/fs/utils/ImmediateFuture.h"

namespace facebook::eden {

FilteredBackingStore::FilteredBackingStore(
    std::shared_ptr<BackingStore> backingStore,
    std::unique_ptr<Filter> filter)
    : backingStore_{std::move(backingStore)}, filter_{std::move(filter)} {};

FilteredBackingStore::~FilteredBackingStore() {}

bool FilteredBackingStore::pathAffectedByFilterChange(
    RelativePathPiece pathOne,
    RelativePathPiece pathTwo,
    folly::StringPiece filterIdOne,
    folly::StringPiece filterIdTwo) {
  auto pathOneIncluded = filter_->isPathFiltered(pathOne, filterIdOne);
  auto pathTwoIncluded = filter_->isPathFiltered(pathTwo, filterIdTwo);
  // If a path is in neither or both filters, then it wouldn't be affected by
  // any change (it is present in both or absent in both).
  if (pathOneIncluded == pathTwoIncluded) {
    return false;
  }

  // If a path is in only 1 filter, it is affected by the change in some way.
  // This function doesn't determine how, just that the path is affected.
  return true;
}

std::tuple<RootId, std::string> parseFilterIdFromRootId(const RootId& rootId) {
  auto rootRange = folly::range(rootId.value());
  auto expectedLength = folly::tryDecodeVarint(rootRange);
  if (UNLIKELY(!expectedLength)) {
    throwf<std::invalid_argument>(
        "Could not decode varint; FilteredBackingStore expects a root ID in the form of <hashLengthVarint><scmHash><filterId>, got {}",
        rootId.value());
  }
  auto root = RootId{std::string{rootRange.begin(), expectedLength.value()}};
  auto filterId = std::string{rootRange.begin() + expectedLength.value()};
  XLOGF(
      DBG7,
      "Decoded Original RootId Length: {}, Original RootId: {}, FilterID: {}",
      expectedLength.value(),
      filterId,
      root.value());
  return {std::move(root), std::move(filterId)};
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

  // It doesn't make sense to compare objects of different types. If this
  // happens, then the caller must be confused. Throw in this case.
  if (typeOne != typeTwo) {
    throwf<std::invalid_argument>(
        "Must compare objects of same type. Attempted to compare: {} vs {}",
        typeOne,
        typeTwo);
  }

  if (typeOne == FilteredObjectId::OBJECT_TYPE_BLOB) {
    // When comparing blob objects, we only need to check if the underlying
    // ObjectIds resolve to equal.
    return backingStore_->compareObjectsById(
        filteredOne.object(), filteredTwo.object());
  }

  // When comparing tree objects, we need to consider filter changes.
  if (typeOne == FilteredObjectId::OBJECT_TYPE_TREE) {
    // If the filters are the same, then we can simply check whether the
    // underlying ObjectIds resolve to equal.
    if (filteredOne.filter() == filteredTwo.filter()) {
      return backingStore_->compareObjectsById(
          filteredOne.object(), filteredTwo.object());
    }

    // If the filters are different, we need to resolve whether the filter
    // change affected the underlying object. This is difficult to do, and is
    // infeasible with the current FilteredBackingStore implementation. Instead,
    // we will return Unknown for any filter changes that we are unsure about.
    //
    // NOTE: If filters are allowed to include regexes in the future, then this
    // may be infeasible to check at all.
    auto pathAffected = pathAffectedByFilterChange(
        filteredOne.path(),
        filteredTwo.path(),
        filteredOne.filter(),
        filteredTwo.filter());
    if (pathAffected) {
      return ObjectComparison::Different;
    } else {
      // If the path wasn't affected by the filter change, we still can't be
      // sure whether a subdirectory of that path was affected. Therefore we
      // must return unknown if the underlying BackingStore reports that the
      // objects are the same.
      //
      // TODO: We could improve this in the future by noting whether a tree has
      // any subdirectories that are affected by filters. There are many ways to
      // do this, but all of them are tricky to do. Let's save this for future
      // optimization.
      auto res = backingStore_->compareObjectsById(
          filteredOne.object(), filteredTwo.object());
      if (res == ObjectComparison::Identical) {
        return ObjectComparison::Unknown;
      } else {
        return res;
      }
    }

  } else {
    // Unknown object type. Throw.
    throwf<std::runtime_error>("Unknown object type: {}", typeOne);
  }
}

PathMap<TreeEntry> FilteredBackingStore::filterImpl(
    const TreePtr unfilteredTree,
    RelativePathPiece treePath,
    folly::StringPiece filterId) {
  auto pathMap = PathMap<TreeEntry>{unfilteredTree->getCaseSensitivity()};
  for (const auto& [path, entry] : *unfilteredTree) {
    auto relPath = RelativePath{treePath} + path;
    if (!filter_->isPathFiltered(relPath.piece(), filterId)) {
      ObjectId oid;
      if (entry.getType() == TreeEntryType::TREE) {
        auto foid =
            FilteredObjectId(relPath.piece(), filterId, entry.getHash());
        oid = ObjectId{foid.getValue()};
      } else {
        auto foid = FilteredObjectId{entry.getHash()};
        oid = ObjectId{foid.getValue()};
      }
      auto treeEntry = TreeEntry{std::move(oid), entry.getType()};
      auto pair = std::pair{path, std::move(treeEntry)};
      pathMap.insert(std::move(pair));
    }
  }
  return pathMap;
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
  return backingStore_->getRootTree(parsedRootId, context)
      .thenValue([filterId = filterId,
                  self = shared_from_this()](GetRootTreeResult rootTreeResult) {
        // apply the filter to the tree
        auto pathMap =
            self->filterImpl(rootTreeResult.tree, RelativePath{""}, filterId);

        auto rootFOID =
            FilteredObjectId{RelativePath{""}, filterId, rootTreeResult.treeId};
        return GetRootTreeResult{
            std::make_shared<const Tree>(
                std::move(pathMap), ObjectId{rootFOID.getValue()}),
            ObjectId{rootFOID.getValue()},
        };
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

folly::SemiFuture<BackingStore::GetTreeResult> FilteredBackingStore::getTree(
    const ObjectId& id,
    const ObjectFetchContextPtr& context) {
  auto filteredId = FilteredObjectId::fromObjectId(id);
  auto unfilteredTree = backingStore_->getTree(filteredId.object(), context);
  return std::move(unfilteredTree)
      .deferValue([self = shared_from_this(),
                   filteredId = std::move(filteredId)](GetTreeResult&& result) {
        auto pathMap = self->filterImpl(
            result.tree, filteredId.path(), filteredId.filter());
        auto tree = std::make_shared<Tree>(
            std::move(pathMap), ObjectId{filteredId.getValue()});
        return GetTreeResult{std::move(tree), result.origin};
      });
}

folly::SemiFuture<BackingStore::GetBlobMetaResult>
FilteredBackingStore::getBlobMetadata(
    const ObjectId& id,
    const ObjectFetchContextPtr& context) {
  auto filteredId = FilteredObjectId::fromObjectId(id);
  return backingStore_->getBlobMetadata(filteredId.object(), context);
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
  std::vector<ObjectId> nonFilteredIds;
  std::transform(ids.begin(), ids.end(), nonFilteredIds.begin(), [](auto id) {
    return FilteredObjectId::fromObjectId(id).object();
  });
  return backingStore_->prefetchBlobs(nonFilteredIds, context);
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

folly::SemiFuture<folly::Unit> FilteredBackingStore::importManifestForRoot(
    const RootId& rootId,
    const Hash20& manifest,
    const ObjectFetchContextPtr& context) {
  // The manifest passed to this function will be unfiltered (i.e. it won't be
  // a FilteredRootId or FilteredObjectId), so we pass it directly to the
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

std::string FilteredBackingStore::renderRootId(const RootId& rootId) {
  auto [underlyingRootId, filterId] = parseFilterIdFromRootId(rootId);
  return createFilteredRootId(
      std::move(underlyingRootId).value(), std::move(filterId));
}

ObjectId FilteredBackingStore::parseObjectId(folly::StringPiece objectId) {
  return backingStore_->parseObjectId(objectId);
}

std::string FilteredBackingStore::renderObjectId(const ObjectId& id) {
  return backingStore_->renderObjectId(id);
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
