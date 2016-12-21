/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "FakeBackingStore.h"

#include <folly/Format.h>
#include <folly/futures/Future.h>
#include <folly/ssl/OpenSSLHash.h>

#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/testharness/TestUtil.h"

using folly::ByteRange;
using folly::Future;
using folly::IOBuf;
using folly::makeFuture;
using folly::StringPiece;
using std::make_unique;
using std::unique_ptr;

namespace facebook {
namespace eden {

FakeBackingStore::FakeBackingStore(std::shared_ptr<LocalStore> localStore)
    : localStore_(std::move(localStore)) {}

FakeBackingStore::~FakeBackingStore() {}

Future<unique_ptr<Tree>> FakeBackingStore::getTree(const Hash& id) {
  auto data = data_.rlock();
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

Future<unique_ptr<Blob>> FakeBackingStore::getBlob(const Hash& id) {
  auto data = data_.rlock();
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
    auto data = data_.rlock();
    auto commitIter = data->commits.find(commitID);
    if (commitIter == data->commits.end()) {
      // Throw immediately, for the same reasons mentioned in getTree()
      throw std::domain_error("commit " + commitID.toString() + " not found");
    }

    storedTreeHash = commitIter->second.get();
  }

  return storedTreeHash->getFuture().then(
      [this, commitID](const std::unique_ptr<Hash>& hash) {
        // Check in the LocalStore for the tree first.
        auto localValue = localStore_->getTree(*hash);
        if (localValue) {
          return makeFuture(std::move(localValue));
        }

        // Next look up the tree in our BackingStore data
        auto data = data_.rlock();
        auto treeIter = data->trees.find(*hash);
        if (treeIter == data->trees.end()) {
          return makeFuture<unique_ptr<Tree>>(std::domain_error(
              "tree " + hash->toString() + " for commit " +
              commitID.toString() + " not found"));
        }

        return treeIter->second->getFuture();
      });
}

StoredBlob* FakeBackingStore::putBlob(StringPiece contents) {
  return putBlob(Hash::sha1(contents), contents);
}

StoredBlob* FakeBackingStore::putBlob(Hash hash, folly::StringPiece contents) {
  auto buf = IOBuf{IOBuf::COPY_BUFFER, ByteRange{contents}};
  auto storedBlob = make_unique<StoredBlob>(Blob(hash, std::move(buf)));

  {
    auto data = data_.wlock();
    auto ret = data->blobs.emplace(hash, std::move(storedBlob));
    if (!ret.second) {
      throw std::domain_error(
          folly::sformat("blob with hash {} already exists", hash.toString()));
    }
    return ret.first->second.get();
  }
}

FakeBackingStore::TreeEntryData::TreeEntryData(
    folly::StringPiece name,
    const Blob& blob,
    mode_t mode)
    : entry{blob.getHash(),
            name,
            FileType::REGULAR_FILE,
            TreeEntry::modeToOwnerPermissions(mode)} {}

FakeBackingStore::TreeEntryData::TreeEntryData(
    folly::StringPiece name,
    const StoredBlob* blob,
    mode_t mode)
    : entry{blob->get().getHash(),
            name,
            FileType::REGULAR_FILE,
            TreeEntry::modeToOwnerPermissions(mode)} {}

FakeBackingStore::TreeEntryData::TreeEntryData(
    folly::StringPiece name,
    const Tree& tree,
    mode_t mode)
    : entry{tree.getHash(),
            name,
            FileType::DIRECTORY,
            TreeEntry::modeToOwnerPermissions(mode)} {}

FakeBackingStore::TreeEntryData::TreeEntryData(
    folly::StringPiece name,
    const StoredTree* tree,
    mode_t mode)
    : entry{tree->get().getHash(),
            name,
            FileType::DIRECTORY,
            TreeEntry::modeToOwnerPermissions(mode)} {}

StoredTree* FakeBackingStore::putTree(
    const std::initializer_list<TreeEntryData>& entryArgs) {
  folly::ssl::OpenSSLHash::Digest digest;
  digest.hash_init(EVP_sha1());

  std::vector<TreeEntry> entries;
  for (const auto& arg : entryArgs) {
    digest.hash_update(ByteRange{arg.entry.getName().stringPiece()});
    digest.hash_update(arg.entry.getHash().getBytes());
    mode_t mode = arg.entry.getMode();
    digest.hash_update(
        ByteRange(reinterpret_cast<const uint8_t*>(&mode), sizeof(mode)));

    entries.push_back(arg.entry);
  }

  Hash::Storage computedHashBytes;
  digest.hash_final(folly::MutableByteRange{computedHashBytes.data(),
                                            computedHashBytes.size()});
  return putTreeImpl(Hash{computedHashBytes}, std::move(entries));
}

StoredTree* FakeBackingStore::putTree(
    Hash hash,
    const std::initializer_list<TreeEntryData>& entryArgs) {
  std::vector<TreeEntry> entries;
  for (const auto& arg : entryArgs) {
    entries.push_back(arg.entry);
  }
  return putTreeImpl(hash, std::move(entries));
}

StoredTree* FakeBackingStore::putTreeImpl(
    Hash hash,
    std::vector<TreeEntry>&& entries) {
  // Sort the entries first
  auto cmpEntry = [](const TreeEntry& a, const TreeEntry& b) {
    return a.getName() < b.getName();
  };
  std::sort(entries.begin(), entries.end(), cmpEntry);

  auto storedTree = make_unique<StoredTree>(Tree{std::move(entries), hash});

  {
    auto data = data_.wlock();
    auto ret = data->trees.emplace(hash, std::move(storedTree));
    if (!ret.second) {
      throw std::domain_error(
          folly::sformat("tree with hash {} already exists", hash.toString()));
    }
    return ret.first->second.get();
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
}
} // facebook::eden
