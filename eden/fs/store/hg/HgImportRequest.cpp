/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/hg/HgImportRequest.h"

#include <folly/Try.h>
#include <folly/futures/Future.h>
#include <folly/futures/Promise.h>

#include "eden/fs/telemetry/RequestMetricsScope.h"

namespace facebook::eden {

template <typename RequestType>
HgImportRequest::HgImportRequest(
    RequestType request,
    ImportPriority priority,
    folly::Promise<typename RequestType::Response>&& promise)
    : request_(std::move(request)),
      priority_(priority),
      promise_(std::move(promise)) {}

template <typename RequestType, typename... Input>
HgImportRequest HgImportRequest::makeRequest(
    ImportPriority priority,
    Input&&... input) {
  auto promise = folly::Promise<typename RequestType::Response>{};
  return HgImportRequest{
      RequestType{std::forward<Input>(input)...}, priority, std::move(promise)};
}

HgImportRequest HgImportRequest::makeBlobImportRequest(
    Hash hash,
    HgProxyHash proxyHash,
    ImportPriority priority) {
  return makeRequest<BlobImport>(priority, hash, std::move(proxyHash));
}

HgImportRequest HgImportRequest::makeTreeImportRequest(
    Hash hash,
    HgProxyHash proxyHash,
    ImportPriority priority,
    bool prefetchMetadata) {
  return makeRequest<TreeImport>(
      priority, hash, std::move(proxyHash), prefetchMetadata);
}

HgImportRequest HgImportRequest::makePrefetchRequest(
    std::vector<HgProxyHash> hashes,
    ImportPriority priority) {
  return makeRequest<Prefetch>(priority, std::move(hashes));
}

} // namespace facebook::eden
