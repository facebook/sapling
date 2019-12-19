/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "FakeBackingStore.h"

#include <folly/Format.h>
#include <folly/MapUtil.h>
#include <folly/futures/Future.h>
#include <folly/logging/xlog.h>
#include <folly/ssl/OpenSSLHash.h>

#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestUtil.h"

using folly::ByteRange;
using folly::Future;
using folly::IOBuf;
using folly::makeFuture;
using folly::SemiFuture;
using folly::StringPiece;
using std::make_unique;
using std::unique_ptr;

namespace facebook {
namespace eden {

FakeBackingStore::FakeBackingStore(std::shared_ptr<LocalStore> localStore)
    : localStore_(std::move(localStore)) {}

FakeBackingStore::~FakeBackingStore() {}

Future<unique_ptr<Tree>> FakeBackingStore::getTree(const Hash& id) {
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
    throw std::domain_error("tree " + id.toString() + " not found");
  }

  return it->second->getFuture();
}

SemiFuture<unique_ptr<Blob>> FakeBackingStore::getBlob(const Hash& id) {
  auto data = data_.wlock();
  ++data->accessCounts[id];
  auto it = data->blobs.find(id);
  if (it == data->blobs.end()) {
    // Throw immediately, for the same reasons mentioned in getTree()
    throw std::domain_error("blob " + id.toString() + " not found");
  }

  return it->second->getFuture();
}

Future<unique_ptr<Tree>> FakeBackingStore::getTreeForCommit(
    const Hash& commitID) {
  StoredHash* storedTreeHash;
  {
    auto data = data_.wlock();
    ++data->accessCounts[commitID];
    auto commitIter = data->commits.find(commitID);
    if (commitIter == data->commits.end()) {
      // Throw immediately, for the same reasons mentioned in getTree()
      throw std::domain_error("commit " + commitID.toString() + " not found");
    }

    storedTreeHash = commitIter->second.get();
  }

  return storedTreeHash->getFuture().thenValue(
      [this, commitID](const std::unique_ptr<Hash>& hash) {
        // Check in the LocalStore for the tree first.
        return getTreeForManifest(commitID, *hash);
      });
}

folly::Future<std::unique_ptr<Tree>> FakeBackingStore::getTreeForManifest(
    const Hash& commitID,
    const Hash& manifestID) {
  // Check in the LocalStore for the tree first.
  return localStore_->getTree(manifestID)
      .thenValue(
          [this, commitID, manifestID](std::unique_ptr<Tree> localValue) {
            if (localValue) {
              return makeFuture(std::move(localValue));
            }

            // Next look up the tree in our BackingStore data
            auto data = data_.rlock();
            auto treeIter = data->trees.find(manifestID);
            if (treeIter == data->trees.end()) {
              return makeFuture<unique_ptr<Tree>>(std::domain_error(
                  "tree " + manifestID.toString() + " for commit " +
                  commitID.toString() + " not found"));
            }

            return treeIter->second->getFuture();
          });
}

Blob FakeBackingStore::makeBlob(folly::StringPiece contents) {
  return makeBlob(Hash::sha1(contents), contents);
}

Blob FakeBackingStore::makeBlob(Hash hash, folly::StringPiece contents) {
  auto buf = IOBuf{IOBuf::COPY_BUFFER, ByteRange{contents}};
  return Blob(hash, std::move(buf));
}

StoredBlob* FakeBackingStore::putBlob(StringPiece contents) {
  return putBlob(Hash::sha1(contents), contents);
}

StoredBlob* FakeBackingStore::putBlob(Hash hash, folly::StringPiece contents) {
  auto ret = maybePutBlob(hash, contents);
  if (!ret.second) {
    throw std::domain_error(
        folly::sformat("blob with hash {} already exists", hash.toString()));
  }
  return ret.first;
}

std::pair<StoredBlob*, bool> FakeBackingStore::maybePutBlob(
    folly::StringPiece contents) {
  return maybePutBlob(Hash::sha1(contents), contents);
}

std::pair<StoredBlob*, bool> FakeBackingStore::maybePutBlob(
    Hash hash,
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
  XLOG(FATAL) << "Unknown fake blob type " << static_cast<int>(type);
}

FakeBackingStore::TreeEntryData::TreeEntryData(
    folly::StringPiece name,
    const Blob& blob,
    FakeBlobType type)
    : entry{blob.getHash(), name, treeEntryTypeFromBlobType(type)} {}

FakeBackingStore::TreeEntryData::TreeEntryData(
    folly::StringPiece name,
    const StoredBlob* blob,
    FakeBlobType type)
    : entry{blob->get().getHash(), name, treeEntryTypeFromBlobType(type)} {}

FakeBackingStore::TreeEntryData::TreeEntryData(
    folly::StringPiece name,
    const Tree& tree)
    : entry{tree.getHash(), name, TreeEntryType::TREE} {}

FakeBackingStore::TreeEntryData::TreeEntryData(
    folly::StringPiece name,
    const StoredTree* tree)
    : entry{tree->get().getHash(), name, TreeEntryType::TREE} {}

StoredTree* FakeBackingStore::putTree(
    const std::initializer_list<TreeEntryData>& entryArgs) {
  auto entries = buildTreeEntries(entryArgs);
  auto hash = computeTreeHash(entries);
  return putTree(hash, entries);
}

StoredTree* FakeBackingStore::putTree(
    Hash hash,
    const std::initializer_list<TreeEntryData>& entryArgs) {
  auto entries = buildTreeEntries(entryArgs);
  return putTreeImpl(hash, std::move(entries));
}

StoredTree* FakeBackingStore::putTree(std::vector<TreeEntry> entries) {
  sortTreeEntries(entries);
  auto hash = computeTreeHash(entries);
  return putTreeImpl(hash, std::move(entries));
}

StoredTree* FakeBackingStore::putTree(
    Hash hash,
    std::vector<TreeEntry> entries) {
  sortTreeEntries(entries);
  return putTreeImpl(hash, std::move(entries));
}

std::pair<StoredTree*, bool> FakeBackingStore::maybePutTree(
    const std::initializer_list<TreeEntryData>& entryArgs) {
  return maybePutTree(buildTreeEntries(entryArgs));
}

std::pair<StoredTree*, bool> FakeBackingStore::maybePutTree(
    std::vector<TreeEntry> entries) {
  sortTreeEntries(entries);
  auto hash = computeTreeHash(entries);
  return maybePutTreeImpl(hash, std::move(entries));
}

std::vector<TreeEntry> FakeBackingStore::buildTreeEntries(
    const std::initializer_list<TreeEntryData>& entryArgs) {
  std::vector<TreeEntry> entries;
  for (const auto& arg : entryArgs) {
    entries.push_back(arg.entry);
  }

  sortTreeEntries(entries);
  return entries;
}

void FakeBackingStore::sortTreeEntries(std::vector<TreeEntry>& entries) {
  auto cmpEntry = [](const TreeEntry& a, const TreeEntry& b) {
    return a.getName() < b.getName();
  };
  std::sort(entries.begin(), entries.end(), cmpEntry);
}

Hash FakeBackingStore::computeTreeHash(
    const std::vector<TreeEntry>& sortedEntries) {
  // Compute a SHA-1 hash over the entry contents.
  // This doesn't match how we generate hashes for either git or mercurial
  // backed stores, but that doesn't really matter.  We only need to be
  // consistent within our own store.
  folly::ssl::OpenSSLHash::Digest digest;
  digest.hash_init(EVP_sha1());

  for (const auto& entry : sortedEntries) {
    digest.hash_update(ByteRange{entry.getName().stringPiece()});
    digest.hash_update(entry.getHash().getBytes());
    mode_t mode = modeFromTreeEntryType(entry.getType());
    digest.hash_update(
        ByteRange(reinterpret_cast<const uint8_t*>(&mode), sizeof(mode)));
  }

  Hash::Storage computedHashBytes;
  digest.hash_final(folly::MutableByteRange{computedHashBytes.data(),
                                            computedHashBytes.size()});
  return Hash{computedHashBytes};
}

StoredTree* FakeBackingStore::putTreeImpl(
    Hash hash,
    std::vector<TreeEntry>&& sortedEntries) {
  auto ret = maybePutTreeImpl(hash, std::move(sortedEntries));
  if (!ret.second) {
    throw std::domain_error(
        folly::sformat("tree with hash {} already exists", hash.toString()));
  }
  return ret.first;
}

std::pair<StoredTree*, bool> FakeBackingStore::maybePutTreeImpl(
    Hash hash,
    std::vector<TreeEntry>&& sortedEntries) {
  auto storedTree =
      make_unique<StoredTree>(Tree{std::move(sortedEntries), hash});

  {
    auto data = data_.wlock();
    auto ret = data->trees.emplace(hash, std::move(storedTree));
    return std::make_pair(ret.first->second.get(), ret.second);
  }
}

StoredHash* FakeBackingStore::putCommit(
    Hash commitHash,
    const StoredTree* tree) {
  return putCommit(commitHash, tree->get().getHash());
}

StoredHash* FakeBackingStore::putCommit(Hash commitHash, Hash treeHash) {
  auto storedHash = make_unique<StoredHash>(treeHash);
  {
    auto data = data_.wlock();
    auto ret = data->commits.emplace(commitHash, std::move(storedHash));
    if (!ret.second) {
      throw std::domain_error(folly::sformat(
          "commit with hash {} already exists", commitHash.toString()));
    }
    return ret.first->second.get();
  }
}

StoredHash* FakeBackingStore::putCommit(
    Hash commitHash,
    const FakeTreeBuilder& builder) {
  return putCommit(commitHash, builder.getRoot()->get().getHash());
}

StoredHash* FakeBackingStore::putCommit(
    folly::StringPiece commitStr,
    const FakeTreeBuilder& builder) {
  return putCommit(makeTestHash(commitStr), builder);
}

StoredTree* FakeBackingStore::getStoredTree(Hash hash) {
  auto data = data_.rlock();
  auto it = data->trees.find(hash);
  if (it == data->trees.end()) {
    throw std::domain_error("stored tree " + hash.toString() + " not found");
  }
  return it->second.get();
}

StoredBlob* FakeBackingStore::getStoredBlob(Hash hash) {
  auto data = data_.rlock();
  auto it = data->blobs.find(hash);
  if (it == data->blobs.end()) {
    throw std::domain_error("stored blob " + hash.toString() + " not found");
  }
  return it->second.get();
}

void FakeBackingStore::discardOutstandingRequests() {
  // Destroying promises before they're complete will trigger a BrokenPromise
  // error, running arbitrary Future callbacks. Take care to destroy the
  // promises outside of the lock.

  std::vector<folly::Promise<std::unique_ptr<Tree>>> trees;
  std::vector<folly::Promise<std::unique_ptr<Blob>>> blobs;
  std::vector<folly::Promise<std::unique_ptr<Hash>>> commits;
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

size_t FakeBackingStore::getAccessCount(const Hash& hash) const {
  return folly::get_default(data_.rlock()->accessCounts, hash, 0);
}
} // namespace eden
} // namespace facebook
