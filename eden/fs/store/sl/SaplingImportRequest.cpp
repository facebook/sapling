/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/sl/SaplingImportRequest.h"

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
    const SlOid& slOid,
    const ObjectFetchContextPtr& context) {
  return makeRequest<BlobImport>(context, slOid);
}

std::shared_ptr<SaplingImportRequest>
SaplingImportRequest::makeTreeImportRequest(
    const SlOid& slOid,
    const ObjectFetchContextPtr& context) {
  return makeRequest<TreeImport>(context, slOid);
}

std::shared_ptr<SaplingImportRequest>
SaplingImportRequest::makeBlobAuxImportRequest(
    const SlOid& slOid,
    const ObjectFetchContextPtr& context) {
  return makeRequest<BlobAuxImport>(context, slOid);
}

std::shared_ptr<SaplingImportRequest>
SaplingImportRequest::makeTreeAuxImportRequest(
    const SlOid& slOid,
    const ObjectFetchContextPtr& context) {
  return makeRequest<TreeAuxImport>(context, slOid);
}

} // namespace facebook::eden
