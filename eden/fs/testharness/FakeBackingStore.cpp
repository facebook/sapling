/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/testharness/FakeBackingStore.h"

#include <fmt/format.h>
#include <folly/MapUtil.h>
#include <folly/futures/Future.h>
#include <folly/logging/xlog.h>
#include <folly/ssl/OpenSSLHash.h>

#include "eden/common/utils/EnumValue.h"
#include "eden/common/utils/FaultInjector.h"
#include "eden/fs/inodes/ServerState.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestUtil.h"

using folly::ByteRange;
using folly::IOBuf;
using folly::makeFuture;
using folly::SemiFuture;
using folly::StringPiece;
using std::make_unique;
using std::unique_ptr;

namespace facebook::eden {
FakeBackingStore::FakeBackingStore(
    LocalStoreCachingPolicy localStoreCachingPolicy,
    std::shared_ptr<ServerState> serverState,
    std::optional<std::string> blake3Key)
    : localStoreCachingPolicy_{localStoreCachingPolicy},
      serverState_{std::move(serverState)},
      blake3Key_(std::move(blake3Key)) {}

FakeBackingStore::~FakeBackingStore() = default;

RootId FakeBackingStore::parseRootId(folly::StringPiece rootId) {
  return RootId{rootId.str()};
}

std::string FakeBackingStore::renderRootId(const RootId& rootId) {
  return rootId.value();
}

ObjectId FakeBackingStore::parseObjectId(folly::StringPiece objectId) {
  return ObjectId{objectId.str()};
}

std::string FakeBackingStore::renderObjectId(const ObjectId& objectId) {
  return objectId.asString();
}

ImmediateFuture<std::shared_ptr<TreeEntry>>
FakeBackingStore::getTreeEntryForObjectId(
    const ObjectId& commitID,
    TreeEntryType treeEntryType,
    const ObjectFetchContextPtr& /* context */) {
  return folly::makeSemiFuture(
      std::make_shared<TreeEntry>(commitID, treeEntryType));
}

ImmediateFuture<BackingStore::GetRootTreeResult> FakeBackingStore::getRootTree(
    const RootId& commitID,
    const ObjectFetchContextPtr& /*context*/) {
  StoredId* storedTreeId;
  {
    auto data = data_.wlock();
    ++data->commitAccessCounts[commitID];
    auto commitIter = data->commits.find(commitID);
    if (commitIter == data->commits.end()) {
      // Throw immediately, for the same reasons mentioned in getTree()
      throw std::domain_error(fmt::format("commit {} not found", commitID));
    }

    storedTreeId = commitIter->second.get();
  }

  return storedTreeId->getFuture()
      .thenValue([this, commitID](const std::shared_ptr<ObjectId>& id) {
        auto data = data_.rlock();
        auto treeIter = data->trees.find(*id);
        if (treeIter == data->trees.end()) {
          return makeImmediateFuture<TreePtr>(std::domain_error(
              fmt::format("tree {} for commit {} not found", *id, commitID)));
        }

        return treeIter->second->getFuture();
      })
      .thenValue([storedTreeId](TreePtr tree) {
        return GetRootTreeResult{tree, storedTreeId->get()};
      })
      .semi();
}

SemiFuture<BackingStore::GetTreeResult> FakeBackingStore::getTree(
    const ObjectId& id,
    const ObjectFetchContextPtr& /*context*/) {
  auto data = data_.wlock();
  ++data->accessCounts[id];
  auto it = data->trees.find(id);
  if (it == data->trees.end()) {
    // Throw immediately, as opposed to returning a Future that contains an
    // exception.  This lets the test code trigger immediate errors in
    // getTree().
    //
    // Delayed errors can be triggered by calling putTree() with a StoredObject
    // and then calling triggerError() later on that object.
    throw std::domain_error(fmt::format("tree {} not found", id));
  }

  return it->second->getFuture()
      .thenValue([](TreePtr tree) {
        return GetTreeResult{
            std::move(tree), ObjectFetchContext::Origin::FromNetworkFetch};
      })
      .semi();
}

SemiFuture<BackingStore::GetTreeAuxResult> FakeBackingStore::getTreeAuxData(
    const ObjectId& /*id*/,
    const ObjectFetchContextPtr& /*context*/) {
  return folly::makeSemiFuture<BackingStore::GetTreeAuxResult>(
      std::domain_error("GetTreeAuxData not implemented for FakeBackingStore"));
}

SemiFuture<BackingStore::GetBlobResult> FakeBackingStore::getBlob(
    const ObjectId& id,
    const ObjectFetchContextPtr& /*context*/) {
  auto data = data_.wlock();
  ++data->accessCounts[id];
  auto it = data->blobs.find(id);
  if (it == data->blobs.end()) {
    // Throw immediately, for the same reasons mentioned in getTree()
    throw std::domain_error(fmt::format("blob {} not found", id));
  }

  return it->second->getFuture()
      .thenValue([](BlobPtr blob) {
        return GetBlobResult{
            std::move(blob), ObjectFetchContext::Origin::FromNetworkFetch};
      })
      .semi();
}

folly::SemiFuture<BackingStore::GetBlobAuxResult>
FakeBackingStore::getBlobAuxData(
    const ObjectId& id,
    const ObjectFetchContextPtr& context) {
  {
    auto data = data_.wlock();
    data->auxDataLookups.push_back(id);
  }

  auto fault = ImmediateFuture<folly::Unit>{std::in_place};
  if (serverState_) {
    fault = serverState_->getFaultInjector().checkAsync("getBlobAuxData", id);
  }

  return std::move(fault)
      .thenValue([this, id, context = context.copy()](auto&&) {
        return ImmediateFuture{getBlob(id, context)};
      })
      .thenValue([this](BackingStore::GetBlobResult result) {
        return BackingStore::GetBlobAuxResult{
            std::make_shared<BlobAuxDataPtr::element_type>(
                Hash20::sha1(result.blob->getContents()),
                blake3Key_ ? Hash32::keyedBlake3(
                                 folly::ByteRange{folly::StringPiece{
                                     blake3Key_->data(), blake3Key_->size()}},
                                 result.blob->getContents())
                           : Hash32::blake3(result.blob->getContents()),
                result.blob->getSize()),
            result.origin};
      })
      .semi();
}

ImmediateFuture<BackingStore::GetGlobFilesResult>
FakeBackingStore::getGlobFiles(
    const RootId& id,
    const std::vector<std::string>& globs,
    const std::vector<std::string>& /*prefixes*/) {
  // Since unordered map can't take a vec for testing purposes only use the
  // first entry in the query
  auto suffixQuery = std::pair<RootId, std::string>(id, globs[0]);
  auto glob = getStoredGlob(suffixQuery)->get();
  return ImmediateFuture<GetGlobFilesResult>{
      GetGlobFilesResult{std::move(glob), id}};
}

Blob FakeBackingStore::makeBlob(folly::StringPiece contents) {
  return Blob{IOBuf{IOBuf::COPY_BUFFER, ByteRange{contents}}};
}

std::pair<StoredBlob*, ObjectId> FakeBackingStore::putBlob(
    StringPiece contents) {
  ObjectId id = ObjectId::sha1(contents);
  return {putBlob(id, contents), id};
}

StoredBlob* FakeBackingStore::putBlob(
    ObjectId id,
    folly::StringPiece contents) {
  auto [storedBlob, oid, inserted] = maybePutBlob(id, contents);
  if (!inserted) {
    throw std::domain_error(fmt::format("blob with id {} already exists", oid));
  }
  return storedBlob;
}

std::tuple<StoredBlob*, ObjectId, bool> FakeBackingStore::maybePutBlob(
    folly::StringPiece contents) {
  return maybePutBlob(ObjectId::sha1(contents), contents);
}

std::tuple<StoredBlob*, ObjectId, bool> FakeBackingStore::maybePutBlob(
    ObjectId id,
    folly::StringPiece contents) {
  auto storedBlob = make_unique<StoredBlob>(makeBlob(contents));

  {
    auto data = data_.wlock();
    auto ret = data->blobs.emplace(id, std::move(storedBlob));
    return std::make_tuple(ret.first->second.get(), std::move(id), ret.second);
  }
}

static TreeEntryType treeEntryTypeFromBlobType(FakeBlobType type) {
  switch (type) {
    case FakeBlobType::REGULAR_FILE:
      return TreeEntryType::REGULAR_FILE;
    case FakeBlobType::EXECUTABLE_FILE:
      return TreeEntryType::EXECUTABLE_FILE;
    case FakeBlobType::SYMLINK:
      return TreeEntryType::SYMLINK;
  }
  XLOGF(FATAL, "Unknown fake blob type {}", enumValue(type));
}

FakeBackingStore::TreeEntryData::TreeEntryData(
    folly::StringPiece name,
    const ObjectId& id,
    FakeBlobType type)
    : entry{
          PathComponent{name},
          TreeEntry{id, treeEntryTypeFromBlobType(type)}} {}

FakeBackingStore::TreeEntryData::TreeEntryData(
    folly::StringPiece name,
    const std::pair<StoredBlob*, ObjectId>& blob,
    FakeBlobType type)
    : entry{
          PathComponent{name},
          TreeEntry{blob.second, treeEntryTypeFromBlobType(type)}} {}

FakeBackingStore::TreeEntryData::TreeEntryData(
    folly::StringPiece name,
    const Tree& tree)
    : entry{
          PathComponent{name},
          TreeEntry{tree.getObjectId(), TreeEntryType::TREE}} {}

FakeBackingStore::TreeEntryData::TreeEntryData(
    folly::StringPiece name,
    const StoredTree* tree)
    : entry{
          PathComponent{name},
          TreeEntry{tree->get().getObjectId(), TreeEntryType::TREE}} {}

StoredTree* FakeBackingStore::putTree(
    const std::initializer_list<TreeEntryData>& entryArgs) {
  auto entries = buildTreeEntries(entryArgs);
  auto id = computeTreeId(entries);
  return putTree(id, entries);
}

StoredTree* FakeBackingStore::putTree(
    ObjectId id,
    const std::initializer_list<TreeEntryData>& entryArgs) {
  auto entries = buildTreeEntries(entryArgs);
  return putTreeImpl(id, std::move(entries));
}

StoredTree* FakeBackingStore::putTree(Tree::container entries) {
  auto id = computeTreeId(entries);
  return putTreeImpl(id, std::move(entries));
}

StoredTree* FakeBackingStore::putTree(ObjectId id, Tree::container entries) {
  return putTreeImpl(id, std::move(entries));
}

std::pair<StoredTree*, bool> FakeBackingStore::maybePutTree(
    const std::initializer_list<TreeEntryData>& entryArgs) {
  return maybePutTree(buildTreeEntries(entryArgs));
}

std::pair<StoredTree*, bool> FakeBackingStore::maybePutTree(
    Tree::container entries) {
  auto id = computeTreeId(entries);
  return maybePutTreeImpl(id, std::move(entries));
}

Tree::container FakeBackingStore::buildTreeEntries(
    const std::initializer_list<TreeEntryData>& entryArgs) {
  Tree::container entries{kPathMapDefaultCaseSensitive};
  for (const auto& arg : entryArgs) {
    entries.insert(arg.entry);
  }

  return entries;
}

ObjectId FakeBackingStore::computeTreeId(const Tree::container& sortedEntries) {
  // Compute a SHA-1 hash over the entry contents.
  // This doesn't match how we generate ids for either git or mercurial
  // backed stores, but that doesn't really matter.  We only need to be
  // consistent within our own store.
  folly::ssl::OpenSSLHash::Digest digest;
  digest.hash_init(EVP_sha1());

  for (const auto& entry : sortedEntries) {
    digest.hash_update(ByteRange{entry.first.view()});
    digest.hash_update(entry.second.getObjectId().getBytes());
    mode_t mode = modeFromTreeEntryType(entry.second.getType());
    digest.hash_update(
        ByteRange(reinterpret_cast<const uint8_t*>(&mode), sizeof(mode)));
  }

  Hash20::Storage computedHashBytes;
  digest.hash_final(folly::MutableByteRange{
      computedHashBytes.data(), computedHashBytes.size()});
  return ObjectId{computedHashBytes};
}

StoredTree* FakeBackingStore::putTreeImpl(
    ObjectId id,
    Tree::container&& sortedEntries) {
  auto ret = maybePutTreeImpl(id, std::move(sortedEntries));
  if (!ret.second) {
    throw std::domain_error(fmt::format("tree with id {} already exists", id));
  }
  return ret.first;
}

std::pair<StoredTree*, bool> FakeBackingStore::maybePutTreeImpl(
    ObjectId id,
    Tree::container&& sortedEntries) {
  auto storedTree = make_unique<StoredTree>(Tree{std::move(sortedEntries), id});

  {
    auto data = data_.wlock();
    auto ret = data->trees.emplace(id, std::move(storedTree));
    return std::make_pair(ret.first->second.get(), ret.second);
  }
}

StoredId* FakeBackingStore::putCommit(
    const RootId& commitId,
    const StoredTree* tree) {
  return putCommit(commitId, tree->get().getObjectId());
}

StoredId* FakeBackingStore::putCommit(const RootId& commitId, ObjectId treeId) {
  auto storedId = make_unique<StoredId>(treeId);
  {
    auto data = data_.wlock();
    auto ret = data->commits.emplace(commitId, std::move(storedId));
    if (!ret.second) {
      throw std::domain_error(
          fmt::format("commit with id {} already exists", commitId));
    }
    return ret.first->second.get();
  }
}

StoredId* FakeBackingStore::putCommit(
    const RootId& commitId,
    const FakeTreeBuilder& builder) {
  return putCommit(commitId, builder.getRoot()->get().getObjectId());
}

StoredId* FakeBackingStore::putCommit(
    folly::StringPiece commitStr,
    const FakeTreeBuilder& builder) {
  return putCommit(RootId(commitStr.str()), builder);
}

StoredGlob* FakeBackingStore::putGlob(
    std::pair<RootId, std::string> suffixQuery,
    std::vector<std::string> contents) {
  auto data = data_.wlock();
  auto storedGlob = std::make_unique<StoredGlob>(std::move(contents));
  auto ret = data->globs.emplace(suffixQuery, std::move(storedGlob));
  if (!ret.second) {
    throw std::domain_error(folly::to<std::string>(
        "glob results for query ", suffixQuery.second, " already exists"));
  }
  return ret.first->second.get();
}

StoredTree* FakeBackingStore::getStoredTree(ObjectId id) {
  auto data = data_.rlock();
  auto it = data->trees.find(id);
  if (it == data->trees.end()) {
    throw std::domain_error(fmt::format("stored tree {} not found", id));
  }
  return it->second.get();
}

StoredBlob* FakeBackingStore::getStoredBlob(ObjectId id) {
  auto data = data_.rlock();
  auto it = data->blobs.find(id);
  if (it == data->blobs.end()) {
    throw std::domain_error(fmt::format("stored blob {} not found", id));
  }
  return it->second.get();
}

StoredGlob* FakeBackingStore::getStoredGlob(
    std::pair<RootId, std::string> suffixQuery) {
  auto data = data_.rlock();
  auto it = data->globs.find(suffixQuery);
  if (it == data->globs.end()) {
    throw std::domain_error(
        fmt::format("stored glob {} not found", suffixQuery));
  }
  return it->second.get();
}

void FakeBackingStore::discardOutstandingRequests() {
  // Destroying promises before they're complete will trigger a BrokenPromise
  // error, running arbitrary Future callbacks. Take care to destroy the
  // promises outside of the lock.

  std::vector<folly::Promise<TreePtr>> trees;
  std::vector<folly::Promise<BlobPtr>> blobs;
  std::vector<folly::Promise<std::shared_ptr<ObjectId>>> commits;
  {
    auto data = data_.wlock();
    for (const auto& tree : data->trees) {
      for (auto&& discarded : tree.second->discardOutstandingRequests()) {
        trees.emplace_back(std::move(discarded));
      }
    }
    for (const auto& blob : data->blobs) {
      for (auto&& discarded : blob.second->discardOutstandingRequests()) {
        blobs.emplace_back(std::move(discarded));
      }
    }
    for (const auto& commit : data->commits) {
      for (auto&& discarded : commit.second->discardOutstandingRequests()) {
        commits.emplace_back(std::move(discarded));
      }
    }
  }
}

size_t FakeBackingStore::getAccessCount(const ObjectId& id) const {
  return folly::get_default(data_.rlock()->accessCounts, id, 0);
}
} // namespace facebook::eden
