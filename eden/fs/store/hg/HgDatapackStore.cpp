/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/hg/HgDatapackStore.h"

#include <folly/Optional.h>
#include <folly/io/IOBuf.h>
#include <folly/logging/xlog.h>
#include <memory>
#include <optional>

#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/store/hg/HgProxyHash.h"
#include "eden/fs/store/hg/ScsProxyHash.h"
#include "eden/fs/utils/Bug.h"

namespace facebook::eden {

namespace {
TreeEntryType fromRawTreeEntryType(RustTreeEntryType type) {
  switch (type) {
    case RustTreeEntryType::RegularFile:
      return TreeEntryType::REGULAR_FILE;
    case RustTreeEntryType::Tree:
      return TreeEntryType::TREE;
    case RustTreeEntryType::ExecutableFile:
      return TreeEntryType::EXECUTABLE_FILE;
    case RustTreeEntryType::Symlink:
      return TreeEntryType::SYMLINK;
  }
  EDEN_BUG() << "unknown tree entry type " << static_cast<uint32_t>(type)
             << " loaded from data store";
}

TreeEntry fromRawTreeEntry(
    RustTreeEntry entry,
    RelativePathPiece path,
    LocalStore::WriteBatch* writeBatch,
    const std::optional<Hash>& commitHash) {
  std::optional<uint64_t> size;
  std::optional<Hash> contentSha1;

  if (entry.size != nullptr) {
    size = *entry.size;
  }

  if (entry.content_sha1 != nullptr) {
    contentSha1 = Hash{*entry.content_sha1};
  }

  auto name = PathComponent(folly::StringPiece{entry.name.asByteRange()});
  auto hash = Hash{entry.hash};

  auto fullPath = path + name;
  auto proxyHash = HgProxyHash::store(fullPath, hash, writeBatch);
  if (commitHash) {
    ScsProxyHash::store(proxyHash, fullPath, commitHash.value(), writeBatch);
  }

  return TreeEntry{
      proxyHash,
      std::move(name),
      fromRawTreeEntryType(entry.ttype),
      size,
      contentSha1};
}

FOLLY_MAYBE_UNUSED std::unique_ptr<Tree> fromRawTree(
    const RustTree* tree,
    const Hash& edenTreeId,
    RelativePathPiece path,
    LocalStore::WriteBatch* writeBatch,
    const std::optional<Hash>& commitHash) {
  std::vector<TreeEntry> entries;

  for (uintptr_t i = 0; i < tree->length; i++) {
    try {
      auto entry =
          fromRawTreeEntry(tree->entries[i], path, writeBatch, commitHash);
      entries.push_back(entry);
    } catch (const PathComponentContainsDirectorySeparator& ex) {
      XLOG(WARN) << "Ignoring directory entry: " << ex.what();
    }
  }

  auto edenTree = std::make_unique<Tree>(std::move(entries), edenTreeId);
  auto serialized = LocalStore::serializeTree(edenTree.get());
  writeBatch->put(
      KeySpace::TreeFamily, edenTreeId, serialized.second.coalesce());
  writeBatch->flush();

  return edenTree;
}
} // namespace

std::unique_ptr<Blob> HgDatapackStore::getBlobLocal(
    const Hash& id,
    const HgProxyHash& hgInfo) {
  auto content = store_.getBlob(
      hgInfo.path().stringPiece(), hgInfo.revHash().getBytes(), true);
  if (content) {
    return std::make_unique<Blob>(id, *content);
  }

  return nullptr;
}

void HgDatapackStore::getBlobBatch(
    const std::vector<Hash>& ids,
    const std::vector<HgProxyHash>& hashes,
    std::vector<folly::Promise<std::unique_ptr<Blob>>*> promises) {
  std::vector<Hash> blobhashes;
  std::vector<std::pair<folly::ByteRange, folly::ByteRange>> requests;

  size_t count = hashes.size();
  requests.reserve(count);
  blobhashes.reserve(count);

  // `.revHash()` will return an owned `Hash` and `getBytes()` will return a
  // reference to that newly created `Hash`. We need to store these `Hash` to
  // avoid storing invalid pointers in `requests`. For a similar reason, we
  // cannot use iterator-based loop here otherwise the reference we get will be
  // pointing to the iterator.
  for (size_t i = 0; i < count; i++) {
    blobhashes.emplace_back(hashes[i].revHash());
  }

  auto blobhash = blobhashes.begin();
  auto hash = hashes.begin();
  for (; blobhash != blobhashes.end(); blobhash++, hash++) {
    XCHECK(hash != hashes.end());
    requests.emplace_back(std::make_pair<>(
        folly::ByteRange{hash->path().stringPiece()}, blobhash->getBytes()));
  }

  store_.getBlobBatch(
      requests,
      false,
      [promises = std::move(promises), ids, requests](
          size_t index, std::unique_ptr<folly::IOBuf> content) {
        XLOGF(
            DBG9,
            "Imported name={} node={}",
            folly::StringPiece{requests[index].first},
            folly::hexlify(requests[index].second));
        auto blob = std::make_unique<Blob>(ids[index], *content);
        promises[index]->setValue(std::move(blob));
      });
}

void HgDatapackStore::getTreeBatch(
    const std::vector<Hash>& ids,
    const std::vector<HgProxyHash>& hashes,
    LocalStore::WriteBatch* writeBatch,
    std::vector<folly::Promise<std::unique_ptr<Tree>>>* promises) {
  std::vector<Hash> treehashes;
  std::vector<std::pair<folly::ByteRange, folly::ByteRange>> requests;

  size_t count = hashes.size();
  requests.reserve(count);
  treehashes.reserve(count);

  // `.revHash()` will return an owned `Hash` and `getBytes()` will return a
  // reference to that newly created `Hash`. We need to store these `Hash` to
  // avoid storing invalid pointers in `requests`. For a similar reason, we
  // cannot use iterator-based loop here otherwise the reference we get will be
  // pointing to the iterator.
  for (size_t i = 0; i < count; i++) {
    treehashes.emplace_back(hashes[i].revHash());
  }

  auto treehash = treehashes.begin();
  auto hash = hashes.begin();
  for (; treehash != treehashes.end(); treehash++, hash++) {
    XCHECK(hash != hashes.end());
    requests.emplace_back(std::make_pair<>(
        folly::ByteRange{hash->path().stringPiece()}, treehash->getBytes()));
  }

  store_.getTreeBatch(
      requests,
      false,
      [promises = promises, ids = ids, hashes = hashes, writeBatch, requests](
          size_t index, std::shared_ptr<RustTree> content) mutable {
        (*promises)[index].setWith([&] {
          XLOGF(
              DBG4,
              "Imported tree name={} node={}",
              folly::StringPiece{requests[index].first},
              folly::hexlify(requests[index].second));
          return fromRawTree(
              content.get(),
              ids[index],
              hashes[index].path(),
              writeBatch,
              std::optional<Hash>());
        });
      });
}

std::unique_ptr<Tree> HgDatapackStore::getTree(
    const RelativePath& path,
    const Hash& manifestId,
    const Hash& edenTreeId,
    LocalStore::WriteBatch* writeBatch,
    const std::optional<Hash>& commitHash) {
  // For root trees we will try getting the tree locally first.  This allows
  // us to catch when Mercurial might have just written a tree to the store,
  // and refresh the store so that the store can pick it up.  We don't do
  // this for all trees, as it would cause a lot of additional work on every
  // cache miss, and just doing it for root trees is sufficient to detect the
  // scenario where Mercurial just wrote a brand new tree.
  bool local_only = path.empty();
  auto tree = store_.getTree(manifestId.getBytes(), local_only);
  if (!tree && local_only) {
    // Mercurial might have just written the tree to the store. Refresh the
    // store and try again, this time allowing remote fetches.
    store_.refresh();
    tree = store_.getTree(manifestId.getBytes(), false);
  }
  if (tree) {
    return fromRawTree(tree.get(), edenTreeId, path, writeBatch, commitHash);
  }
  return nullptr;
}

void HgDatapackStore::refresh() {
  store_.refresh();
}

} // namespace facebook::eden
