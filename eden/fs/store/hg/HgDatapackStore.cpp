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

#include "eden/common/utils/Bug.h"
#include "eden/common/utils/FaultInjector.h"
#include "eden/common/utils/RefPtr.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/store/hg/HgImportRequest.h"
#include "eden/fs/store/hg/HgProxyHash.h"
#include "eden/fs/telemetry/LogEvent.h"
#include "eden/fs/telemetry/StructuredLogger.h"

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

bool doFilteredPathsApply(
    bool ignoreFilteredPathsConfig,
    const std::unordered_set<RelativePath>& filteredPaths,
    const RelativePath& path) {
  return ignoreFilteredPathsConfig || filteredPaths.empty() ||
      filteredPaths.count(path) == 0;
}

TreePtr fromRawTree(
    const sapling::Tree* tree,
    const ObjectId& edenTreeId,
    RelativePathPiece path,
    HgObjectIdFormat hgObjectIdFormat,
    const std::unordered_set<RelativePath>& filteredPaths,
    bool ignoreFilteredPathsConfig) {
  Tree::container entries{kPathMapDefaultCaseSensitive};

  entries.reserve(tree->entries.size());
  for (uintptr_t i = 0; i < tree->entries.size(); i++) {
    try {
      auto entry = fromRawTreeEntry(tree->entries[i], path, hgObjectIdFormat);
      // TODO(xavierd): In the case where this checks becomes too hot, we may
      // need to change to a Trie like datastructure for fast filtering.
      if (doFilteredPathsApply(
              ignoreFilteredPathsConfig, filteredPaths, path + entry.first)) {
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
  auto manifestNode = store_->getManifestNode(commitId.getBytes());
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
  store_->getTreeBatch(
      folly::range(requests),
      false,
      // store_->getTreeBatch is blocking, hence we can take these by reference.
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
                store_->getRepoName(),
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
                    *filteredPaths,
                    runtimeOptions_->ignoreConfigFilter())};
              });
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
  store_->flush();
}

} // namespace facebook::eden
