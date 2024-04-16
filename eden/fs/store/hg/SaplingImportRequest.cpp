/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/hg/SaplingImportRequest.h"

#include <folly/Try.h>
#include <folly/futures/Promise.h>

namespace facebook::eden {

template <typename RequestType>
SaplingImportRequest::SaplingImportRequest(
    RequestType request,
    ImportPriority priority,
    ObjectFetchContext::Cause cause,
    OptionalProcessId pid,
    folly::Promise<typename RequestType::Response>&& promise)
    : request_(std::move(request)),
      priority_(priority),
      cause_(cause),
      pid_(pid),
      promise_(std::move(promise)) {}

template <typename RequestType, typename... Input>
std::shared_ptr<SaplingImportRequest> SaplingImportRequest::makeRequest(
    ImportPriority priority,
    ObjectFetchContext::Cause cause,
    OptionalProcessId pid,
    Input&&... input) {
  auto promise = folly::Promise<typename RequestType::Response>{};
  return std::make_shared<SaplingImportRequest>(
      RequestType{std::forward<Input>(input)...},
      priority,
      cause,
      pid,
      std::move(promise));
}

std::shared_ptr<SaplingImportRequest>
SaplingImportRequest::makeBlobImportRequest(
    const ObjectId& hash,
    const HgProxyHash& proxyHash,
    ImportPriority priority,
    ObjectFetchContext::Cause cause,
    OptionalProcessId pid) {
  return makeRequest<BlobImport>(priority, cause, pid, hash, proxyHash);
}

std::shared_ptr<SaplingImportRequest>
SaplingImportRequest::makeTreeImportRequest(
    const ObjectId& hash,
    const HgProxyHash& proxyHash,
    ImportPriority priority,
    ObjectFetchContext::Cause cause,
    OptionalProcessId pid) {
  return makeRequest<TreeImport>(priority, cause, pid, hash, proxyHash);
}

std::shared_ptr<SaplingImportRequest>
SaplingImportRequest::makeBlobMetaImportRequest(
    const ObjectId& hash,
    const HgProxyHash& proxyHash,
    ImportPriority priority,
    ObjectFetchContext::Cause cause,
    OptionalProcessId pid) {
  return makeRequest<BlobMetaImport>(priority, cause, pid, hash, proxyHash);
}

} // namespace facebook::eden
