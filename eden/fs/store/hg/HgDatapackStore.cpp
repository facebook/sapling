/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
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

#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/store/hg/HgImportRequest.h"
#include "eden/fs/store/hg/HgProxyHash.h"
#include "eden/fs/utils/Bug.h"

namespace facebook::eden {

namespace {

TreeEntryType fromRawTreeEntryType(sapling::TreeEntryType type) {
  switch (type) {
    case sapling::TreeEntryType::RegularFile:
      return TreeEntryType::REGULAR_FILE;
    case sapling::TreeEntryType::Tree:
      return TreeEntryType::TREE;
    case sapling::TreeEntryType::ExecutableFile:
      return TreeEntryType::EXECUTABLE_FILE;
    case sapling::TreeEntryType::Symlink:
      return TreeEntryType::SYMLINK;
  }
  EDEN_BUG() << "unknown tree entry type " << static_cast<uint32_t>(type)
             << " loaded from data store";
}

Tree::value_type fromRawTreeEntry(
    sapling::TreeEntry entry,
    RelativePathPiece path,
    HgObjectIdFormat hgObjectIdFormat) {
  std::optional<uint64_t> size;
  std::optional<Hash20> contentSha1;

  if (entry.size != nullptr) {
    size = *entry.size;
  }

  if (entry.content_sha1 != nullptr) {
    contentSha1 = Hash20{*entry.content_sha1};
  }

  auto name = PathComponent(folly::StringPiece{entry.name.asByteRange()});
  auto hash = Hash20{entry.hash};

  auto fullPath = path + name;
  auto proxyHash = HgProxyHash::store(fullPath, hash, hgObjectIdFormat);

  auto treeEntry = TreeEntry{
      proxyHash, fromRawTreeEntryType(entry.ttype), size, contentSha1};
  return {std::move(name), std::move(treeEntry)};
}

std::unique_ptr<Tree> fromRawTree(
    const sapling::Tree* tree,
    const ObjectId& edenTreeId,
    RelativePathPiece path,
    HgObjectIdFormat hgObjectIdFormat) {
  Tree::container entries{kPathMapDefaultCaseSensitive};

  entries.reserve(tree->length);
  for (uintptr_t i = 0; i < tree->length; i++) {
    try {
      auto entry = fromRawTreeEntry(tree->entries[i], path, hgObjectIdFormat);
      entries.emplace(entry.first, std::move(entry.second));
    } catch (const PathComponentContainsDirectorySeparator& ex) {
      XLOG(WARN) << "Ignoring directory entry: " << ex.what();
    }
  }
  return std::make_unique<Tree>(std::move(entries), edenTreeId);
}

} // namespace

void HgDatapackStore::getTreeBatch(
    const std::vector<std::shared_ptr<HgImportRequest>>& importRequests) {
  auto count = importRequests.size();

  std::vector<sapling::NodeId> requests;
  requests.reserve(count);

  for (const auto& importRequest : importRequests) {
    auto& proxyHash =
        importRequest->getRequest<HgImportRequest::TreeImport>()->proxyHash;
    requests.emplace_back(proxyHash.byteHash());
  }
  std::vector<RequestMetricsScope> requestsWatches;
  requestsWatches.reserve(count);

  for (auto i = 0ul; i < count; i++) {
    requestsWatches.emplace_back(&liveBatchedTreeWatches_);
  }

  auto hgObjectIdFormat = config_->getEdenConfig()->hgObjectIdFormat.getValue();

  store_.getTreeBatch(
      folly::range(requests),
      false,
      // store_.getTreeBatch is blocking, hence we can take these by reference.
      [&](size_t index,
          const folly::Try<std::shared_ptr<sapling::Tree>>& content) mutable {
        if (content.hasException()) {
          // TODO: Do something with this error.
          return;
        }
        XLOGF(DBG4, "Imported tree node={}", folly::hexlify(requests[index]));
        auto& importRequest = importRequests[index];
        auto* treeRequest =
            importRequest->getRequest<HgImportRequest::TreeImport>();

        auto tree = fromRawTree(
            content.value().get(),
            treeRequest->hash,
            treeRequest->proxyHash.path(),
            hgObjectIdFormat);

        importRequest->getPromise<std::unique_ptr<Tree>>()->setValue(
            std::move(tree));

        // Make sure that we're stopping this watch.
        auto watch = std::move(requestsWatches[index]);
      });
}

std::unique_ptr<Tree> HgDatapackStore::getTree(
    const RelativePath& path,
    const Hash20& manifestId,
    const ObjectId& edenTreeId) {
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
    store_.flush();
    tree = store_.getTree(manifestId.getBytes(), false);
  }
  if (tree) {
    auto hgObjectIdFormat =
        config_->getEdenConfig()->hgObjectIdFormat.getValue();
    return fromRawTree(tree.get(), edenTreeId, path, hgObjectIdFormat);
  }
  return nullptr;
}

std::unique_ptr<Tree> HgDatapackStore::getTreeLocal(
    const ObjectId& edenTreeId,
    const HgProxyHash& proxyHash) {
  auto tree = store_.getTree(proxyHash.byteHash(), /*local=*/true);
  auto hgObjectIdFormat = config_->getEdenConfig()->hgObjectIdFormat.getValue();
  if (tree) {
    return fromRawTree(
        tree.get(), edenTreeId, proxyHash.path(), hgObjectIdFormat);
  }

  return nullptr;
}

void HgDatapackStore::getBlobBatch(
    const std::vector<std::shared_ptr<HgImportRequest>>& importRequests) {
  size_t count = importRequests.size();

  std::vector<sapling::NodeId> requests;
  requests.reserve(count);

  for (const auto& importRequest : importRequests) {
    auto& proxyHash =
        importRequest->getRequest<HgImportRequest::BlobImport>()->proxyHash;
    requests.emplace_back(proxyHash.byteHash());
  }

  std::vector<RequestMetricsScope> requestsWatches;
  requestsWatches.reserve(count);

  for (auto i = 0ul; i < count; i++) {
    requestsWatches.emplace_back(&liveBatchedBlobWatches_);
  }

  store_.getBlobBatch(
      folly::range(requests),
      false,
      // store_.getBlobBatch is blocking, hence we can take these by reference.
      [&](size_t index, const folly::Try<std::unique_ptr<folly::IOBuf>>& content) {
        if (content.hasException()) {
          // TODO: Do something with this error.
          return;
        }

        XLOGF(DBG9, "Imported node={}", folly::hexlify(requests[index]));
        auto& importRequest = importRequests[index];
        auto* blobRequest =
            importRequest->getRequest<HgImportRequest::BlobImport>();
        auto blob = std::make_unique<Blob>(blobRequest->hash, *content.value());
        importRequest->getPromise<std::unique_ptr<Blob>>()->setValue(
            std::move(blob));

        // Make sure that we're stopping this watch.
        auto watch = std::move(requestsWatches[index]);
      });
}

std::unique_ptr<Blob> HgDatapackStore::getBlobLocal(
    const ObjectId& id,
    const HgProxyHash& hgInfo) {
  auto content = store_.getBlob(hgInfo.byteHash(), true);
  if (content) {
    return std::make_unique<Blob>(id, std::move(*content));
  }

  return nullptr;
}

std::unique_ptr<BlobMetadata> HgDatapackStore::getLocalBlobMetadata(
    const Hash20& id) {
  auto metadata = store_.getBlobMetadata(id.getBytes(), true /*local_only*/);
  if (metadata) {
    return std::make_unique<BlobMetadata>(
        BlobMetadata{Hash20{metadata->content_sha1}, metadata->total_size});
  }
  return nullptr;
}

void HgDatapackStore::flush() {
  store_.flush();
}

} // namespace facebook::eden
