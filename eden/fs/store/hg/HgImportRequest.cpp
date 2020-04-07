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

namespace facebook {
namespace eden {

namespace {
template <typename Request, typename Input>
std::pair<HgImportRequest, folly::SemiFuture<typename Request::Response>>
makeRequest(Input&& input, ImportPriority priority) {
  auto [promise, future] =
      folly::makePromiseContract<typename Request::Response>();
  return std::make_pair(
      HgImportRequest{
          Request{std::forward<Input>(input)}, priority, std::move(promise)},
      std::move(future));
}
} // namespace

std::pair<HgImportRequest, folly::SemiFuture<std::unique_ptr<Blob>>>
HgImportRequest::makeBlobImportRequest(Hash hash, ImportPriority priority) {
  return makeRequest<BlobImport>(hash, priority);
}

std::pair<HgImportRequest, folly::SemiFuture<std::unique_ptr<Tree>>>
HgImportRequest::makeTreeImportRequest(Hash hash, ImportPriority priority) {
  return makeRequest<TreeImport>(hash, priority);
}

std::pair<HgImportRequest, folly::SemiFuture<folly::Unit>>
HgImportRequest::makePrefetchRequest(
    std::vector<Hash> hashes,
    ImportPriority priority) {
  return makeRequest<Prefetch>(hashes, priority);
}

} // namespace eden
} // namespace facebook
