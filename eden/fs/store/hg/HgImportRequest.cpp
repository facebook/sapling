/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
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
std::shared_ptr<HgImportRequest> HgImportRequest::makeRequest(
    ImportPriority priority,
    Input&&... input) {
  auto promise = folly::Promise<typename RequestType::Response>{};
  return std::make_shared<HgImportRequest>(
      RequestType{std::forward<Input>(input)...}, priority, std::move(promise));
}

std::shared_ptr<HgImportRequest> HgImportRequest::makeBlobImportRequest(
    ObjectId hash,
    HgProxyHash proxyHash,
    ImportPriority priority) {
  return makeRequest<BlobImport>(priority, hash, std::move(proxyHash));
}

std::shared_ptr<HgImportRequest> HgImportRequest::makeTreeImportRequest(
    ObjectId hash,
    HgProxyHash proxyHash,
    ImportPriority priority) {
  return makeRequest<TreeImport>(priority, hash, std::move(proxyHash));
}

std::shared_ptr<HgImportRequest> HgImportRequest::makePrefetchRequest(
    std::vector<HgProxyHash> hashes,
    ImportPriority priority) {
  return makeRequest<Prefetch>(priority, std::move(hashes));
}

} // namespace facebook::eden
