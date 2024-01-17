/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/hg/HgDatapackStore.h"

#include <folly/Range.h>
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
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/store/hg/HgImportRequest.h"
#include "eden/fs/store/hg/HgProxyHash.h"
#include "eden/fs/telemetry/LogEvent.h"
#include "eden/fs/telemetry/StructuredLogger.h"
#include "eden/fs/utils/Bug.h"
#include "eden/fs/utils/FaultInjector.h"
#include "eden/fs/utils/RefPtr.h"

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

  if (entry.has_size) {
    size = entry.size;
  }

  if (entry.has_sha1) {
    contentSha1.emplace(Hash20(std::move(entry.content_sha1)));
  }

  if (entry.has_blake3) {
    contentBlake3.emplace(Hash32(std::move(entry.content_blake3)));
  }

  auto name = PathComponent(folly::StringPiece{
      folly::ByteRange{entry.name.data(), entry.name.size()}});
  Hash20 hash(std::move(entry.hash));

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

  entries.reserve(tree->entries.size());
  for (uintptr_t i = 0; i < tree->entries.size(); i++) {
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

std::optional<Hash20> HgDatapackStore::getManifestNode(
    const ObjectId& commitId) {
  auto manifestNode = store_.getManifestNode(commitId.getBytes());
  if (!manifestNode.has_value()) {
    XLOGF(DBG2, "Error while getting manifest node from datapackstore");
    return std::nullopt;
  }
  return Hash20(*std::move(manifestNode));
}

void HgDatapackStore::getTreeBatch(const ImportRequestsList& importRequests) {
  auto preparedRequests =
      prepareRequests<HgImportRequest::TreeImport>(importRequests, "Tree");
  auto importRequestsMap = std::move(preparedRequests.first);
  auto requests = std::move(preparedRequests.second);
  auto hgObjectIdFormat = config_->getEdenConfig()->hgObjectIdFormat.getValue();
  const auto filteredPaths =
      config_->getEdenConfig()->hgFilteredPaths.getValue();

  faultInjector_.check("HgDatapackStore::getTreeBatch", "");
  store_.getTreeBatch(
      folly::range(requests),
      false,
      // store_.getTreeBatch is blocking, hence we can take these by reference.
      [&, filteredPaths](
          size_t index,
          folly::Try<std::shared_ptr<sapling::Tree>> content) mutable {
        if (content.hasException()) {
          XLOGF(
              DBG6,
              "Failed to import node={} from EdenAPI (batch tree {}/{}): {}",
              folly::hexlify(requests[index]),
              index,
              requests.size(),
              content.exception().what().toStdString());
        } else {
          XLOGF(
              DBG6,
              "Imported node={} from EdenAPI (batch tree: {}/{})",
              folly::hexlify(requests[index]),
              index,
              requests.size());
        }

        if (content.hasException()) {
          if (logger_) {
            logger_->logEvent(FetchMiss{
                store_.getRepoName(),
                FetchMiss::Tree,
                content.exception().what().toStdString(),
                false});
          }

          return;
        }

        XLOGF(DBG9, "Imported Tree node={}", folly::hexlify(requests[index]));
        const auto& nodeId = requests[index];
        auto& [importRequestList, watch] = importRequestsMap[nodeId];
        for (auto& importRequest : importRequestList) {
          auto* treeRequest =
              importRequest->getRequest<HgImportRequest::TreeImport>();
          importRequest->getPromise<TreePtr>()->setWith(
              [&]() -> folly::Try<TreePtr> {
                if (content.hasException()) {
                  return folly::Try<TreePtr>{content.exception()};
                }
                return folly::Try{fromRawTree(
                    content.value().get(),
                    treeRequest->hash,
                    treeRequest->proxyHash.path(),
                    hgObjectIdFormat,
                    *filteredPaths)};
              });
        }

        // Make sure that we're stopping this watch.
        watch.reset();
      });
}

folly::Try<TreePtr> HgDatapackStore::getTree(
    const RelativePath& path,
    const Hash20& manifestId,
    const ObjectId& edenTreeId,
    const ObjectFetchContextPtr& /*context*/) {
  // For root trees we will try getting the tree locally first.  This allows
  // us to catch when Mercurial might have just written a tree to the store,
  // and refresh the store so that the store can pick it up.  We don't do
  // this for all trees, as it would cause a lot of additional work on every
  // cache miss, and just doing it for root trees is sufficient to detect the
  // scenario where Mercurial just wrote a brand new tree.
  bool local_only = path.empty();
  auto tree = store_.getTree(
      manifestId.getBytes(),
      local_only /*, sapling::ClientRequestInfo(context)*/);
  if (tree.hasException() && local_only) {
    // Mercurial might have just written the tree to the store. Refresh the
    // store and try again, this time allowing remote fetches.
    store_.flush();
    tree = store_.getTree(
        manifestId.getBytes(), false /*, sapling::ClientRequestInfo(context)*/);
  }

  using GetTreeResult = folly::Try<TreePtr>;

  if (tree.hasValue()) {
    auto hgObjectIdFormat =
        config_->getEdenConfig()->hgObjectIdFormat.getValue();
    const auto filteredPaths =
        config_->getEdenConfig()->hgFilteredPaths.getValue();
    return GetTreeResult{fromRawTree(
        tree.value().get(),
        edenTreeId,
        path,
        std::move(hgObjectIdFormat),
        std::move(*filteredPaths))};
  } else {
    return GetTreeResult{tree.exception()};
  }
}

TreePtr HgDatapackStore::getTreeLocal(
    const ObjectId& edenTreeId,
    const HgProxyHash& proxyHash) {
  auto tree = store_.getTree(proxyHash.byteHash(), /*local=*/true);
  if (tree.hasValue()) {
    auto hgObjectIdFormat =
        config_->getEdenConfig()->hgObjectIdFormat.getValue();
    const auto filteredPaths =
        config_->getEdenConfig()->hgFilteredPaths.getValue();
    return fromRawTree(
        tree.value().get(),
        edenTreeId,
        proxyHash.path(),
        hgObjectIdFormat,
        *filteredPaths);
  }

  return nullptr;
}

void HgDatapackStore::getBlobBatch(const ImportRequestsList& importRequests) {
  auto preparedRequests =
      prepareRequests<HgImportRequest::BlobImport>(importRequests, "Blob");
  auto importRequestsMap = std::move(preparedRequests.first);
  auto requests = std::move(preparedRequests.second);

  store_.getBlobBatch(
      folly::range(requests),
      false,
      // store_.getBlobBatch is blocking, hence we can take these by reference.
      [&](size_t index, folly::Try<std::unique_ptr<folly::IOBuf>> content) {
        if (content.hasException()) {
          XLOGF(
              DBG6,
              "Failed to import node={} from EdenAPI (batch {}/{}): {}",
              folly::hexlify(requests[index]),
              index,
              requests.size(),
              content.exception().what().toStdString());
        } else {
          XLOGF(
              DBG6,
              "Imported node={} from EdenAPI (batch: {}/{})",
              folly::hexlify(requests[index]),
              index,
              requests.size());
        }

        if (content.hasException()) {
          if (logger_) {
            logger_->logEvent(FetchMiss{
                store_.getRepoName(),
                FetchMiss::Blob,
                content.exception().what().toStdString(),
                false});
          }

          return;
        }

        XLOGF(DBG9, "Imported Blob node={}", folly::hexlify(requests[index]));
        const auto& nodeId = requests[index];
        auto& [importRequestList, watch] = importRequestsMap[nodeId];
        auto result = content.hasException()
            ? folly::Try<BlobPtr>{content.exception()}
            : folly::Try{
                  std::make_shared<BlobPtr::element_type>(*content.value())};
        for (auto& importRequest : importRequestList) {
          importRequest->getPromise<BlobPtr>()->setWith(
              [&]() -> folly::Try<BlobPtr> { return result; });
        }

        // Make sure that we're stopping this watch.
        watch.reset();
      });
}

folly::Try<BlobPtr> HgDatapackStore::getBlob(
    const HgProxyHash& hgInfo,
    bool localOnly) {
  auto blob = store_.getBlob(hgInfo.byteHash(), localOnly);

  using GetBlobResult = folly::Try<BlobPtr>;

  if (blob.hasValue()) {
    return GetBlobResult{
        std::make_shared<BlobPtr::element_type>(std::move(*blob.value()))};
  } else {
    return GetBlobResult{blob.exception()};
  }
}

folly::Try<BlobMetadataPtr> HgDatapackStore::getLocalBlobMetadata(
    const HgProxyHash& hgInfo) {
  auto metadata =
      store_.getBlobMetadata(hgInfo.byteHash(), true /*local_only*/);

  using GetBlobMetadataResult = folly::Try<BlobMetadataPtr>;

  if (metadata.hasValue()) {
    std::optional<Hash32> blake3;
    if (metadata.value()->has_blake3) {
      blake3.emplace(Hash32{std::move(metadata.value()->content_blake3)});
    }
    return GetBlobMetadataResult{
        std::make_shared<BlobMetadataPtr::element_type>(BlobMetadata{
            Hash20{std::move(metadata.value()->content_sha1)},
            blake3,
            metadata.value()->total_size})};
  } else {
    return GetBlobMetadataResult{metadata.exception()};
  }
}

void HgDatapackStore::getBlobMetadataBatch(
    const ImportRequestsList& importRequests) {
  auto preparedRequests = prepareRequests<HgImportRequest::BlobMetaImport>(
      importRequests, "BlobMetadata");
  auto importRequestsMap = std::move(preparedRequests.first);
  auto requests = std::move(preparedRequests.second);

  store_.getBlobMetadataBatch(
      folly::range(requests),
      false,
      // store_.getBlobMetadataBatch is blocking, hence we can take these by
      // reference.
      [&](size_t index,
          folly::Try<std::shared_ptr<sapling::FileAuxData>> auxTry) {
        if (auxTry.hasException()) {
          XLOGF(
              DBG6,
              "Failed to import metadata node={} from EdenAPI (batch {}/{}): {}",
              folly::hexlify(requests[index]),
              index,
              requests.size(),
              auxTry.exception().what().toStdString());
        } else {
          XLOGF(
              DBG6,
              "Imported metadata node={} from EdenAPI (batch: {}/{})",
              folly::hexlify(requests[index]),
              index,
              requests.size());
        }

        if (auxTry.hasException()) {
          if (logger_) {
            logger_->logEvent(FetchMiss{
                store_.getRepoName(),
                FetchMiss::BlobMetadata,
                auxTry.exception().what().toStdString(),
                false});
          }

          return;
        }

        XLOGF(
            DBG9, "Imported BlobMetadata={}", folly::hexlify(requests[index]));
        const auto& nodeId = requests[index];
        auto& [importRequestList, watch] = importRequestsMap[nodeId];
        folly::Try<BlobMetadataPtr> result;
        if (auxTry.hasException()) {
          result = folly::Try<BlobMetadataPtr>{auxTry.exception()};
        } else {
          auto& aux = auxTry.value();
          std::optional<Hash32> blake3;
          if (aux->has_blake3) {
            blake3.emplace(Hash32{std::move(aux->content_blake3)});
          }

          result = folly::Try{std::make_shared<BlobMetadataPtr::element_type>(
              Hash20{std::move(aux->content_sha1)}, blake3, aux->total_size)};
        }
        for (auto& importRequest : importRequestList) {
          importRequest->getPromise<BlobMetadataPtr>()->setWith(
              [&]() -> folly::Try<BlobMetadataPtr> { return result; });
        }

        // Make sure that we're stopping this watch.
        watch.reset();
      });
}

template <typename T>
std::pair<HgDatapackStore::ImportRequestsMap, std::vector<sapling::NodeId>>
HgDatapackStore::prepareRequests(
    const ImportRequestsList& importRequests,
    const std::string& requestType) {
  // TODO: extract each ClientRequestInfo from importRequests into a
  // sapling::ClientRequestInfo and pass them with the corresponding
  // sapling::NodeId

  // Group requests by proxyHash to ensure no duplicates in fetch request to
  // SaplingNativeBackingStore.
  ImportRequestsMap importRequestsMap;
  for (const auto& importRequest : importRequests) {
    auto nodeId = importRequest->getRequest<T>()->proxyHash.byteHash();

    // Look for and log duplicates.
    const auto& importRequestsEntry = importRequestsMap.find(nodeId);
    if (importRequestsEntry != importRequestsMap.end()) {
      XLOGF(
          DBG9,
          "Duplicate {} fetch request with proxyHash: {}",
          requestType,
          nodeId);
      auto& importRequestList = importRequestsEntry->second.first;

      // Only look for mismatched requests if logging level is high enough.
      // Make sure this level is the same as the XLOG_IF statement below.
      if (XLOG_IS_ON(DBG9)) {
        // Log requests that do not have the same hash (ObjectId).
        // This happens when two paths (file or directory) have same content.
        for (const auto& priorRequest : importRequestList) {
          XLOGF_IF(
              DBG9,
              UNLIKELY(
                  priorRequest->template getRequest<T>()->hash !=
                  importRequest->getRequest<T>()->hash),
              "{} requests have the same proxyHash (HgProxyHash) but different hash (ObjectId). "
              "This should not happen. Previous request: hash='{}', proxyHash='{}', proxyHash.path='{}'; "
              "current request: hash='{}', proxyHash ='{}', proxyHash.path='{}'.",
              requestType,
              priorRequest->template getRequest<T>()->hash.asHexString(),
              folly::hexlify(
                  priorRequest->template getRequest<T>()->proxyHash.byteHash()),
              priorRequest->template getRequest<T>()->proxyHash.path(),
              importRequest->getRequest<T>()->hash.asHexString(),
              folly::hexlify(
                  importRequest->getRequest<T>()->proxyHash.byteHash()),
              importRequest->getRequest<T>()->proxyHash.path());
        }
      }

      importRequestList.emplace_back(importRequest);
    } else {
      std::vector<std::shared_ptr<HgImportRequest>> requests({importRequest});
      importRequestsMap.emplace(
          nodeId, make_pair(requests, &liveBatchedBlobWatches_));
    }
  }

  // Indexable vector of nodeIds - required by SaplingNativeBackingStore API.
  std::vector<sapling::NodeId> requests;
  requests.reserve(importRequestsMap.size());
  std::transform(
      importRequestsMap.begin(),
      importRequestsMap.end(),
      std::back_inserter(requests),
      [](auto& pair) { return pair.first; });

  return std::make_pair(std::move(importRequestsMap), std::move(requests));
}

void HgDatapackStore::flush() {
  store_.flush();
}

} // namespace facebook::eden
