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

namespace facebook {
namespace eden {

namespace {
template <typename Request, typename... Input>
std::pair<HgImportRequest, folly::SemiFuture<typename Request::Response>>
makeRequest(
    ImportPriority priority,
    std::unique_ptr<RequestMetricsScope> metricsScope,
    Input&&... input) {
  auto [promise, future] =
      folly::makePromiseContract<typename Request::Response>();
  return std::make_pair(
      HgImportRequest{
          Request{std::forward<Input>(input)...}, priority, std::move(promise)},
      std::move(future).defer(
          [metrics = std::move(metricsScope)](auto&& result) {
            return std::forward<decltype(result)>(result);
          }));
}
} // namespace

std::pair<HgImportRequest, folly::SemiFuture<std::unique_ptr<Blob>>>
HgImportRequest::makeBlobImportRequest(
    Hash hash,
    HgProxyHash proxyHash,
    ImportPriority priority,
    std::unique_ptr<RequestMetricsScope> metricsScope) {
  return makeRequest<BlobImport>(
      priority, std::move(metricsScope), hash, std::move(proxyHash));
}

std::pair<HgImportRequest, folly::SemiFuture<std::unique_ptr<Tree>>>
HgImportRequest::makeTreeImportRequest(
    Hash hash,
    HgProxyHash proxyHash,
    ImportPriority priority,
    std::unique_ptr<RequestMetricsScope> metricsScope,
    bool prefetchMetadata) {
  return makeRequest<TreeImport>(
      priority,
      std::move(metricsScope),
      hash,
      std::move(proxyHash),
      prefetchMetadata);
}

std::pair<HgImportRequest, folly::SemiFuture<folly::Unit>>
HgImportRequest::makePrefetchRequest(
    std::vector<HgProxyHash> hashes,
    ImportPriority priority,
    std::unique_ptr<RequestMetricsScope> metricsScope) {
  return makeRequest<Prefetch>(
      priority, std::move(metricsScope), std::move(hashes));
}
} // namespace eden
} // namespace facebook
