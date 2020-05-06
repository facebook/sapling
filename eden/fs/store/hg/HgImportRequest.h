/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/futures/Promise.h>
#include <utility>
#include <variant>

#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/ImportPriority.h"
#include "eden/fs/telemetry/RequestMetricsScope.h"
#include "eden/fs/utils/Bug.h"

namespace facebook {
namespace eden {

/**
 * Represents an Hg import request. This class contains all the necessary
 * information needed to fulfill the request as well as a promise that will be
 * resolved after the requested data is imported.
 */
class HgImportRequest {
 public:
  struct BlobImport {
    using Response = std::unique_ptr<Blob>;

    Hash hash;
  };

  struct TreeImport {
    using Response = std::unique_ptr<Tree>;

    Hash hash;
  };

  struct Prefetch {
    using Response = folly::Unit;

    std::vector<Hash> hashes;
  };

  HgImportRequest(HgImportRequest&&) = default;
  HgImportRequest& operator=(HgImportRequest&&) = default;

  HgImportRequest(const HgImportRequest&) = delete;
  HgImportRequest& operator=(const HgImportRequest&) = delete;

  static std::pair<HgImportRequest, folly::SemiFuture<std::unique_ptr<Blob>>>
  makeBlobImportRequest(
      Hash hash,
      ImportPriority priority,
      std::unique_ptr<RequestMetricsScope> metricsScope);

  static std::pair<HgImportRequest, folly::SemiFuture<std::unique_ptr<Tree>>>
  makeTreeImportRequest(
      Hash hash,
      ImportPriority priority,
      std::unique_ptr<RequestMetricsScope> metricsScope);

  static std::pair<HgImportRequest, folly::SemiFuture<folly::Unit>>
  makePrefetchRequest(
      std::vector<Hash> hashes,
      ImportPriority priority,
      std::unique_ptr<RequestMetricsScope> metricsScope);

  template <typename RequestType>
  HgImportRequest(
      RequestType request,
      ImportPriority priority,
      folly::Promise<typename RequestType::Response>&& promise)
      : request_(std::move(request)),
        priority_(priority),
        promise_(std::move(promise)) {}

  template <typename T>
  const T* getRequest() noexcept {
    return std::get_if<T>(&request_);
  }

  /**
   * Set the inner Promise with the result of the function.
   */
  template <typename T, typename Func>
  void setWith(Func func) {
    auto promise = std::get_if<folly::Promise<typename T::Response>>(&promise_);

    if (!promise) {
      EDEN_BUG() << "invalid promise type";
    }

    promise->setWith([func = std::move(func)]() { return func(); });
  }

 private:
  using Request = std::variant<BlobImport, TreeImport, Prefetch>;
  using Response = std::variant<
      folly::Promise<std::unique_ptr<Blob>>,
      folly::Promise<std::unique_ptr<Tree>>,
      folly::Promise<folly::Unit>>;

  Request request_;
  ImportPriority priority_;
  Response promise_;

  friend bool operator<(
      const HgImportRequest& lhs,
      const HgImportRequest& rhs) {
    return lhs.priority_ < rhs.priority_;
  }
};

} // namespace eden
} // namespace facebook
