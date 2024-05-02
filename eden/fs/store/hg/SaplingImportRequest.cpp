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
    const ObjectFetchContextPtr& context,
    folly::Promise<typename RequestType::Response>&& promise)
    : request_(std::move(request)),
      context_(context.copy()),
      priority_(context_->getPriority()),
      promise_(std::move(promise)) {}

template <typename RequestType, typename... Input>
std::shared_ptr<SaplingImportRequest> SaplingImportRequest::makeRequest(
    const ObjectFetchContextPtr& context,
    Input&&... input) {
  auto promise = folly::Promise<typename RequestType::Response>{};
  return std::make_shared<SaplingImportRequest>(
      RequestType{std::forward<Input>(input)...}, context, std::move(promise));
}

std::shared_ptr<SaplingImportRequest>
SaplingImportRequest::makeBlobImportRequest(
    const ObjectId& hash,
    const HgProxyHash& proxyHash,
    const ObjectFetchContextPtr& context) {
  return makeRequest<BlobImport>(context, hash, proxyHash);
}

std::shared_ptr<SaplingImportRequest>
SaplingImportRequest::makeTreeImportRequest(
    const ObjectId& hash,
    const HgProxyHash& proxyHash,
    const ObjectFetchContextPtr& context) {
  return makeRequest<TreeImport>(context, hash, proxyHash);
}

std::shared_ptr<SaplingImportRequest>
SaplingImportRequest::makeBlobMetaImportRequest(
    const ObjectId& hash,
    const HgProxyHash& proxyHash,
    const ObjectFetchContextPtr& context) {
  return makeRequest<BlobMetaImport>(context, hash, proxyHash);
}

} // namespace facebook::eden
