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
  auto [promise, future] = folly::makePromiseContract<ResponseType>();

  return std::make_pair(
      HgImportRequest{RequestType::BlobImport,
                      std::move(hash),
                      priority,
                      std::move(promise)},
      std::move(future).deferValue(
          [](auto result) -> folly::SemiFuture<std::unique_ptr<Blob>> {
            if (auto* blob = std::get_if<std::unique_ptr<Blob>>(&result)) {
              return folly::makeSemiFuture<>(std::move(*blob));
            }
            return folly::makeSemiFuture<std::unique_ptr<Blob>>(
                std::runtime_error("invalid response"));
          }));
}

std::pair<HgImportRequest, folly::SemiFuture<std::unique_ptr<Tree>>>
HgImportRequest::makeTreeImportRequest(Hash hash, ImportPriority priority) {
  auto [promise, future] = folly::makePromiseContract<ResponseType>();

  return std::make_pair(
      HgImportRequest{RequestType::TreeImport,
                      std::move(hash),
                      priority,
                      std::move(promise)},
      std::move(future).deferValue(
          [](auto result) -> folly::SemiFuture<std::unique_ptr<Tree>> {
            if (auto* tree = std::get_if<std::unique_ptr<Tree>>(&result)) {
              return folly::makeSemiFuture<>(std::move(*tree));
            }
            return folly::makeSemiFuture<std::unique_ptr<Tree>>(
                std::runtime_error("invalid response"));
          }));
}

} // namespace eden
} // namespace facebook
