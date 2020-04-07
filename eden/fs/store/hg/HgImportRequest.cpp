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

std::pair<HgImportRequest, folly::SemiFuture<std::unique_ptr<Blob>>>
HgImportRequest::makeBlobImportRequest(Hash hash, ImportPriority priority) {
  auto [promise, future] = folly::makePromiseContract<std::unique_ptr<Blob>>();

  return std::make_pair(
      HgImportRequest{
          BlobImport{std::move(hash)}, priority, std::move(promise)},
      std::move(future));
}

std::pair<HgImportRequest, folly::SemiFuture<std::unique_ptr<Tree>>>
HgImportRequest::makeTreeImportRequest(Hash hash, ImportPriority priority) {
  auto [promise, future] = folly::makePromiseContract<std::unique_ptr<Tree>>();

  return std::make_pair(
      HgImportRequest{
          TreeImport{std::move(hash)}, priority, std::move(promise)},
      std::move(future));
}

} // namespace eden
} // namespace facebook
