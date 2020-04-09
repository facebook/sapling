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
      folly::Promise<typename RequestType::Response>&& promise,
      std::unique_ptr<RequestMetricsScope> metricsScope)
      : request_(std::move(request)),
        priority_(priority),
        promise_(std::move(promise)),
        metrics_(std::move(metricsScope)) {}

  template <typename T>
  const T* getRequest() noexcept {
    return std::get_if<T>(&request_);
  }

  /**
   * Set Try with the Future for the inner promise.
   *
   * We need this method instead of letting the caller directly call
   * `promise.setTry()` because of the use of `std::variant`,
   * `promise.setTry` won't be able to convert the incoming response to the
   * correct `std::variant` automatically.
   */
  template <typename T>
  void setTry(folly::Try<T> result) {
    auto promise = std::get_if<folly::Promise<T>>(&promise_); // Promise<T>

    if (!promise) {
      EDEN_BUG() << "invalid promise type";
    }

    if (result.hasValue()) {
      promise->setValue(std::move(result.value()));
    } else {
      promise->setException(std::move(result.exception()));
    }
  }

  /**
   * Returns pointer for the RequestMetricsScope that is
   * tracking this request, this should be used to transfer
   * ownership of this scope when the request is removed from
   * the queue, and the import is processed.
   */
  std::unique_ptr<RequestMetricsScope> getOwnershipOfImportTracker();

 private:
  using Request = std::variant<BlobImport, TreeImport, Prefetch>;
  using Response = std::variant<
      folly::Promise<std::unique_ptr<Blob>>,
      folly::Promise<std::unique_ptr<Tree>>,
      folly::Promise<folly::Unit>>;

  Request request_;
  ImportPriority priority_;
  Response promise_;
  std::unique_ptr<RequestMetricsScope> metrics_;

  friend bool operator<(
      const HgImportRequest& lhs,
      const HgImportRequest& rhs) {
    return lhs.priority_ < rhs.priority_;
  }
};

} // namespace eden
} // namespace facebook
