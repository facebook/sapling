/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "FakeBackingStore.h"

#include <fmt/format.h>
#include <folly/MapUtil.h>
#include <folly/futures/Future.h>
#include <folly/logging/xlog.h>
#include <folly/ssl/OpenSSLHash.h>

#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestUtil.h"
#include "eden/fs/utils/EnumValue.h"

using folly::ByteRange;
using folly::IOBuf;
using folly::makeFuture;
using folly::SemiFuture;
using folly::StringPiece;
using std::make_unique;
using std::unique_ptr;

namespace facebook::eden {

FakeBackingStore::FakeBackingStore() = default;

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

ImmediateFuture<std::unique_ptr<TreeEntry>>
FakeBackingStore::getTreeEntryForObjectId(
    const ObjectId& commitID,
    TreeEntryType treeEntryType,
    const ObjectFetchContextPtr& /* context */) {
  return folly::makeSemiFuture(
      std::make_unique<TreeEntry>(commitID, treeEntryType));
}

ImmediateFuture<unique_ptr<Tree>> FakeBackingStore::getRootTree(
    const RootId& commitID,
    const ObjectFetchContextPtr& /*context*/) {
  StoredHash* storedTreeHash;
  {
    auto data = data_.wlock();
    ++data->commitAccessCounts[commitID];
    auto commitIter = data->commits.find(commitID);
    if (commitIter == data->commits.end()) {
      // Throw immediately, for the same reasons mentioned in getTree()
      throw std::domain_error(
          folly::to<std::string>("commit ", commitID, " not found"));
    }

    storedTreeHash = commitIter->second.get();
  }

  return storedTreeHash->getFuture()
      .thenValue([this, commitID](const std::unique_ptr<ObjectId>& hash) {
        auto data = data_.rlock();
        auto treeIter = data->trees.find(*hash);
        if (treeIter == data->trees.end()) {
          return makeFuture<unique_ptr<Tree>>(
              std::domain_error(folly::to<std::string>(
                  "tree ", *hash, " for commit ", commitID, " not found")));
        }

        return treeIter->second->getFuture();
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

  return it->second->getFuture().thenValue([](std::unique_ptr<Tree> tree) {
    return GetTreeResult{
        std::move(tree), ObjectFetchContext::Origin::FromNetworkFetch};
  });
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

  return it->second->getFuture().thenValue([](std::unique_ptr<Blob> blob) {
    return GetBlobResult{
        std::move(blob), ObjectFetchContext::Origin::FromNetworkFetch};
  });
}

Blob FakeBackingStore::makeBlob(folly::StringPiece contents) {
  return makeBlob(ObjectId::sha1(contents), contents);
}

Blob FakeBackingStore::makeBlob(ObjectId hash, folly::StringPiece contents) {
  auto buf = IOBuf{IOBuf::COPY_BUFFER, ByteRange{contents}};
  return Blob(hash, std::move(buf));
}

StoredBlob* FakeBackingStore::putBlob(StringPiece contents) {
  return putBlob(ObjectId::sha1(contents), contents);
}

StoredBlob* FakeBackingStore::putBlob(
    ObjectId hash,
    folly::StringPiece contents) {
  auto ret = maybePutBlob(hash, contents);
  if (!ret.second) {
    throw std::domain_error(
        fmt::format("blob with hash {} already exists", hash));
  }
  return ret.first;
}

std::pair<StoredBlob*, bool> FakeBackingStore::maybePutBlob(
    folly::StringPiece contents) {
  return maybePutBlob(ObjectId::sha1(contents), contents);
}

std::pair<StoredBlob*, bool> FakeBackingStore::maybePutBlob(
    ObjectId hash,
    folly::StringPiece contents) {
  auto storedBlob = make_unique<StoredBlob>(makeBlob(hash, contents));

  {
    auto data = data_.wlock();
    auto ret = data->blobs.emplace(hash, std::move(storedBlob));
    return std::make_pair(ret.first->second.get(), ret.second);
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
  XLOG(FATAL) << "Unknown fake blob type " << enumValue(type);
}

FakeBackingStore::TreeEntryData::TreeEntryData(
    folly::StringPiece name,
    const Blob& blob,
    FakeBlobType type)
    : entry{
          PathComponent{name},
          TreeEntry{blob.getHash(), treeEntryTypeFromBlobType(type)}} {}

FakeBackingStore::TreeEntryData::TreeEntryData(
    folly::StringPiece name,
    const StoredBlob* blob,
    FakeBlobType type)
    : entry{
          PathComponent{name},
          TreeEntry{blob->get().getHash(), treeEntryTypeFromBlobType(type)}} {}

FakeBackingStore::TreeEntryData::TreeEntryData(
    folly::StringPiece name,
    const Tree& tree)
    : entry{
          PathComponent{name},
          TreeEntry{tree.getHash(), TreeEntryType::TREE}} {}

FakeBackingStore::TreeEntryData::TreeEntryData(
    folly::StringPiece name,
    const StoredTree* tree)
    : entry{
          PathComponent{name},
          TreeEntry{tree->get().getHash(), TreeEntryType::TREE}} {}

StoredTree* FakeBackingStore::putTree(
    const std::initializer_list<TreeEntryData>& entryArgs) {
  auto entries = buildTreeEntries(entryArgs);
  auto hash = computeTreeHash(entries);
  return putTree(hash, entries);
}

StoredTree* FakeBackingStore::putTree(
    ObjectId hash,
    const std::initializer_list<TreeEntryData>& entryArgs) {
  auto entries = buildTreeEntries(entryArgs);
  return putTreeImpl(hash, std::move(entries));
}

StoredTree* FakeBackingStore::putTree(Tree::container entries) {
  auto hash = computeTreeHash(entries);
  return putTreeImpl(hash, std::move(entries));
}

StoredTree* FakeBackingStore::putTree(ObjectId hash, Tree::container entries) {
  return putTreeImpl(hash, std::move(entries));
}

std::pair<StoredTree*, bool> FakeBackingStore::maybePutTree(
    const std::initializer_list<TreeEntryData>& entryArgs) {
  return maybePutTree(buildTreeEntries(entryArgs));
}

std::pair<StoredTree*, bool> FakeBackingStore::maybePutTree(
    Tree::container entries) {
  auto hash = computeTreeHash(entries);
  return maybePutTreeImpl(hash, std::move(entries));
}

Tree::container FakeBackingStore::buildTreeEntries(
    const std::initializer_list<TreeEntryData>& entryArgs) {
  Tree::container entries{kPathMapDefaultCaseSensitive};
  for (const auto& arg : entryArgs) {
    entries.insert(arg.entry);
  }

  return entries;
}

ObjectId FakeBackingStore::computeTreeHash(
    const Tree::container& sortedEntries) {
  // Compute a SHA-1 hash over the entry contents.
  // This doesn't match how we generate hashes for either git or mercurial
  // backed stores, but that doesn't really matter.  We only need to be
  // consistent within our own store.
  folly::ssl::OpenSSLHash::Digest digest;
  digest.hash_init(EVP_sha1());

  for (const auto& entry : sortedEntries) {
    digest.hash_update(ByteRange{entry.first.view()});
    digest.hash_update(entry.second.getHash().getBytes());
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
    ObjectId hash,
    Tree::container&& sortedEntries) {
  auto ret = maybePutTreeImpl(hash, std::move(sortedEntries));
  if (!ret.second) {
    throw std::domain_error(
        fmt::format("tree with hash {} already exists", hash));
  }
  return ret.first;
}

std::pair<StoredTree*, bool> FakeBackingStore::maybePutTreeImpl(
    ObjectId hash,
    Tree::container&& sortedEntries) {
  auto storedTree =
      make_unique<StoredTree>(Tree{std::move(sortedEntries), hash});

  {
    auto data = data_.wlock();
    auto ret = data->trees.emplace(hash, std::move(storedTree));
    return std::make_pair(ret.first->second.get(), ret.second);
  }
}

StoredHash* FakeBackingStore::putCommit(
    const RootId& commitHash,
    const StoredTree* tree) {
  return putCommit(commitHash, tree->get().getHash());
}

StoredHash* FakeBackingStore::putCommit(
    const RootId& commitHash,
    ObjectId treeHash) {
  auto storedHash = make_unique<StoredHash>(treeHash);
  {
    auto data = data_.wlock();
    auto ret = data->commits.emplace(commitHash, std::move(storedHash));
    if (!ret.second) {
      throw std::domain_error(folly::to<std::string>(
          "commit with hash ", commitHash, " already exists"));
    }
    return ret.first->second.get();
  }
}

StoredHash* FakeBackingStore::putCommit(
    const RootId& commitHash,
    const FakeTreeBuilder& builder) {
  return putCommit(commitHash, builder.getRoot()->get().getHash());
}

StoredHash* FakeBackingStore::putCommit(
    folly::StringPiece commitStr,
    const FakeTreeBuilder& builder) {
  return putCommit(RootId(commitStr.str()), builder);
}

StoredTree* FakeBackingStore::getStoredTree(ObjectId hash) {
  auto data = data_.rlock();
  auto it = data->trees.find(hash);
  if (it == data->trees.end()) {
    throw std::domain_error(fmt::format("stored tree {} not found", hash));
  }
  return it->second.get();
}

StoredBlob* FakeBackingStore::getStoredBlob(ObjectId hash) {
  auto data = data_.rlock();
  auto it = data->blobs.find(hash);
  if (it == data->blobs.end()) {
    throw std::domain_error(fmt::format("stored blob {} not found", hash));
  }
  return it->second.get();
}

void FakeBackingStore::discardOutstandingRequests() {
  // Destroying promises before they're complete will trigger a BrokenPromise
  // error, running arbitrary Future callbacks. Take care to destroy the
  // promises outside of the lock.

  std::vector<folly::Promise<std::unique_ptr<Tree>>> trees;
  std::vector<folly::Promise<std::unique_ptr<Blob>>> blobs;
  std::vector<folly::Promise<std::unique_ptr<ObjectId>>> commits;
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

size_t FakeBackingStore::getAccessCount(const ObjectId& hash) const {
  return folly::get_default(data_.rlock()->accessCounts, hash, 0);
}
} // namespace facebook::eden
