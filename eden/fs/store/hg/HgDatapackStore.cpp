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
#include "eden/fs/model/BlobMetadata.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/store/hg/HgImportRequest.h"
#include "eden/fs/store/hg/HgProxyHash.h"
#include "eden/fs/telemetry/LogEvent.h"
#include "eden/fs/telemetry/StructuredLogger.h"
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
  std::optional<Hash32> contentBlake3;

  if (entry.size != nullptr) {
    size = *entry.size;
  }

  if (entry.content_sha1 != nullptr) {
    contentSha1.emplace(*entry.content_sha1);
  }

  if (entry.content_blake3 != nullptr) {
    contentBlake3.emplace(*entry.content_blake3);
  }

  auto name = PathComponent(folly::StringPiece{entry.name.asByteRange()});
  auto hash = Hash20{entry.hash};

  auto fullPath = path + name;
  auto proxyHash = HgProxyHash::store(fullPath, hash, hgObjectIdFormat);

  auto treeEntry = TreeEntry{
      proxyHash,
      fromRawTreeEntryType(entry.ttype),
      size,
      contentSha1,
      contentBlake3};
  return {std::move(name), std::move(treeEntry)};
}

TreePtr fromRawTree(
    const sapling::Tree* tree,
    const ObjectId& edenTreeId,
    RelativePathPiece path,
    HgObjectIdFormat hgObjectIdFormat,
    const std::unordered_set<RelativePath>& filteredPaths) {
  Tree::container entries{kPathMapDefaultCaseSensitive};

  entries.reserve(tree->length);
  for (uintptr_t i = 0; i < tree->length; i++) {
    try {
      auto entry = fromRawTreeEntry(tree->entries[i], path, hgObjectIdFormat);
      // TODO(xavierd): In the case where this checks becomes too hot, we may
      // need to change to a Trie like datastructure for fast filtering.
      if (filteredPaths.empty() ||
          filteredPaths.count(path + entry.first) == 0) {
        entries.emplace(entry.first, std::move(entry.second));
      }
    } catch (const PathComponentContainsDirectorySeparator& ex) {
      XLOG(WARN) << "Ignoring directory entry: " << ex.what();
    }
  }
  return std::make_shared<TreePtr::element_type>(
      std::move(entries), edenTreeId);
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
  const auto& filteredPaths =
      config_->getEdenConfig()->hgFilteredPaths.getValue();

  store_.getTreeBatch(
      folly::range(requests),
      false,
      // store_.getTreeBatch is blocking, hence we can take these by reference.
      [&](size_t index,
          folly::Try<std::shared_ptr<sapling::Tree>> content) mutable {
        if (config_->getEdenConfig()->hgTreeFetchFallback.getValue() &&
            content.hasException()) {
          if (logger_) {
            logger_->logEvent(EdenApiMiss{
                repoName_,
                EdenApiMiss::Tree,
                content.exception().what().toStdString()});
          }

          // If we're falling back, the caller will fulfill this Promise with a
          // tree from HgImporter.
          // TODO: Remove this.
          return;
        }
        XLOGF(DBG4, "Imported tree node={}", folly::hexlify(requests[index]));
        auto& importRequest = importRequests[index];
        auto* treeRequest =
            importRequest->getRequest<HgImportRequest::TreeImport>();
        // A proposed folly::Try::and_then would make the following much
        // simpler.
        importRequest->getPromise<TreePtr>()->setWith(
            [&]() -> folly::Try<TreePtr> {
              if (content.hasException()) {
                return folly::Try<TreePtr>{std::move(content).exception()};
              }
              return folly::Try{fromRawTree(
                  content.value().get(),
                  treeRequest->hash,
                  treeRequest->proxyHash.path(),
                  hgObjectIdFormat,
                  filteredPaths)};
            });

        // Make sure that we're stopping this watch.
        requestsWatches[index].reset();
      });
}

TreePtr HgDatapackStore::getTree(
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
    const auto& filteredPaths =
        config_->getEdenConfig()->hgFilteredPaths.getValue();
    return fromRawTree(
        tree.get(), edenTreeId, path, hgObjectIdFormat, filteredPaths);
  }
  return nullptr;
}

TreePtr HgDatapackStore::getTreeLocal(
    const ObjectId& edenTreeId,
    const HgProxyHash& proxyHash) {
  auto tree = store_.getTree(proxyHash.byteHash(), /*local=*/true);
  if (tree) {
    auto hgObjectIdFormat =
        config_->getEdenConfig()->hgObjectIdFormat.getValue();
    const auto& filteredPaths =
        config_->getEdenConfig()->hgFilteredPaths.getValue();
    return fromRawTree(
        tree.get(),
        edenTreeId,
        proxyHash.path(),
        hgObjectIdFormat,
        filteredPaths);
  }

  return nullptr;
}

void HgDatapackStore::getBlobBatch(
    const std::vector<std::shared_ptr<HgImportRequest>>& importRequests) {
  size_t count = importRequests.size();

  std::vector<sapling::NodeId> requests;
  requests.reserve(count);
  for (const auto& importRequest : importRequests) {
    requests.emplace_back(
        importRequest->getRequest<HgImportRequest::BlobImport>()
            ->proxyHash.byteHash());
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
      [&](size_t index, folly::Try<std::unique_ptr<folly::IOBuf>> content) {
        if (config_->getEdenConfig()->hgBlobFetchFallback.getValue() &&
            content.hasException()) {
          if (logger_) {
            logger_->logEvent(EdenApiMiss{
                repoName_,
                EdenApiMiss::Blob,
                content.exception().what().toStdString()});
          }

          // If we're falling back, the caller will fulfill this Promise with a
          // blob from HgImporter.
          // TODO: Remove this.
          return;
        }

        XLOGF(DBG9, "Imported node={}", folly::hexlify(requests[index]));
        auto& importRequest = importRequests[index];
        // A proposed folly::Try::and_then would make the following much
        // simpler.
        importRequest->getPromise<BlobPtr>()->setWith(
            [&]() -> folly::Try<BlobPtr> {
              if (content.hasException()) {
                return folly::Try<BlobPtr>{std::move(content).exception()};
              }
              return folly::Try{
                  std::make_shared<BlobPtr::element_type>(*content.value())};
            });

        // Make sure that we're stopping this watch.
        requestsWatches[index].reset();
      });
}

BlobPtr HgDatapackStore::getBlobLocal(const HgProxyHash& hgInfo) {
  auto content = store_.getBlob(hgInfo.byteHash(), true);
  if (content) {
    return std::make_shared<BlobPtr::element_type>(std::move(*content));
  }

  return nullptr;
}

BlobMetadataPtr HgDatapackStore::getLocalBlobMetadata(
    const HgProxyHash& hgInfo) {
  auto metadata =
      store_.getBlobMetadata(hgInfo.byteHash(), true /*local_only*/);
  if (metadata) {
    std::optional<Hash32> blake3;
    if (metadata->content_blake3 != nullptr) {
      blake3.emplace(*metadata->content_blake3);
    }
    return std::make_shared<BlobMetadataPtr::element_type>(BlobMetadata{
        Hash20{metadata->content_sha1}, blake3, metadata->total_size});
  }
  return nullptr;
}

void HgDatapackStore::getBlobMetadataBatch(
    const std::vector<std::shared_ptr<HgImportRequest>>& importRequests) {
  size_t count = importRequests.size();

  std::vector<sapling::NodeId> requests;
  requests.reserve(count);
  for (const auto& importRequest : importRequests) {
    requests.emplace_back(
        importRequest->getRequest<HgImportRequest::BlobMetaImport>()
            ->proxyHash.byteHash());
  }

  std::vector<RequestMetricsScope> requestsWatches;
  requestsWatches.reserve(count);
  for (auto i = 0ul; i < count; i++) {
    requestsWatches.emplace_back(&liveBatchedBlobMetaWatches_);
  }

  store_.getBlobMetadataBatch(
      folly::range(requests),
      false,
      [&](size_t index,
          folly::Try<std::shared_ptr<sapling::FileAuxData>> auxTry) {
        if (auxTry.hasException() &&
            config_->getEdenConfig()->hgBlobMetaFetchFallback.getValue()) {
          // The caller will fallback to fetching the blob.
          // TODO: Remove this.
          return;
        }

        XLOGF(DBG9, "Imported aux={}", folly::hexlify(requests[index]));
        auto& importRequest = importRequests[index];
        importRequest->getPromise<BlobMetadataPtr>()->setWith(
            [&]() -> folly::Try<BlobMetadataPtr> {
              if (auxTry.hasException()) {
                return folly::Try<BlobMetadataPtr>{
                    std::move(auxTry).exception()};
              }

              auto& aux = auxTry.value();
              std::optional<Hash32> blake3;
              if (aux->content_blake3 != nullptr) {
                blake3.emplace(*aux->content_blake3);
              }

              return folly::Try{std::make_shared<BlobMetadataPtr::element_type>(
                  Hash20{aux->content_sha1}, blake3, aux->total_size)};
            });

        // Make sure that we're stopping this watch.
        requestsWatches[index].reset();
      });
}

void HgDatapackStore::flush() {
  store_.flush();
}

} // namespace facebook::eden
