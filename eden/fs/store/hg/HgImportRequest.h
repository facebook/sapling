/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
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
#include "eden/fs/store/hg/HgProxyHash.h"
#include "eden/fs/telemetry/RequestMetricsScope.h"
#include "eden/fs/utils/Bug.h"
#include "eden/fs/utils/IDGen.h"

namespace facebook::eden {

/**
 * Represents an Hg import request. This class contains all the necessary
 * information needed to fulfill the request as well as a promise that will be
 * resolved after the requested data is imported. Blobs and Trees also contain
 * a vector of promises to fulfill, corresponding to duplicate requests
 */
class HgImportRequest {
 public:
  struct BlobImport {
    using Response = std::unique_ptr<Blob>;
    BlobImport(ObjectId hash, HgProxyHash proxyHash)
        : hash(hash), proxyHash(proxyHash) {}

    ObjectId hash;
    HgProxyHash proxyHash;

    // In the case where requests de-duplicate to this one, the requests
    // promise will be enqueued to the following vector.
    std::vector<folly::Promise<Response>> promises;
  };

  struct TreeImport {
    using Response = std::unique_ptr<Tree>;
    TreeImport(ObjectId hash, HgProxyHash proxyHash)
        : hash(hash), proxyHash(proxyHash) {}

    ObjectId hash;
    HgProxyHash proxyHash;

    // See the comment above for BlobImport::promises
    std::vector<folly::Promise<Response>> promises;
  };

  /**
   * Allocate a blob request.
   */
  static std::shared_ptr<HgImportRequest> makeBlobImportRequest(
      ObjectId hash,
      HgProxyHash proxyHash,
      ImportPriority priority);

  /**
   * Allocate a tree request.
   */
  static std::shared_ptr<HgImportRequest> makeTreeImportRequest(
      ObjectId hash,
      HgProxyHash proxyHash,
      ImportPriority priority);

  /**
   * Implementation detail of the make*Request functions from above. Do not use
   * directly.
   */
  template <typename RequestType>
  HgImportRequest(
      RequestType request,
      ImportPriority priority,
      folly::Promise<typename RequestType::Response>&& promise);

  ~HgImportRequest() = default;

  HgImportRequest(HgImportRequest&&) = default;
  HgImportRequest& operator=(HgImportRequest&&) = default;

  template <typename T>
  T* getRequest() noexcept {
    return std::get_if<T>(&request_);
  }

  template <typename T>
  bool isType() const noexcept {
    return std::holds_alternative<T>(request_);
  }

  size_t getType() const noexcept {
    return request_.index();
  }

  ImportPriority getPriority() const noexcept {
    return priority_;
  }

  void setPriority(ImportPriority priority) noexcept {
    priority_ = priority;
  }

  template <typename T>
  folly::Promise<T>* getPromise() {
    auto promise = std::get_if<folly::Promise<T>>(&promise_); // Promise<T>

    if (!promise) {
      EDEN_BUG() << "invalid promise type";
    }
    return promise;
  }

  uint64_t getUnique() const {
    return unique_;
  }

  std::chrono::steady_clock::time_point getRequestTime() const {
    return requestTime_;
  }

 private:
  /**
   * Implementation detail of the various make*Request functions.
   */
  template <typename Request, typename... Input>
  static std::shared_ptr<HgImportRequest> makeRequest(
      ImportPriority priority,
      Input&&... input);

  HgImportRequest(const HgImportRequest&) = delete;
  HgImportRequest& operator=(const HgImportRequest&) = delete;

  using Request = std::variant<BlobImport, TreeImport>;
  using Response = std::variant<
      folly::Promise<std::unique_ptr<Blob>>,
      folly::Promise<std::unique_ptr<Tree>>>;

  Request request_;
  ImportPriority priority_;
  Response promise_;
  uint64_t unique_ = generateUniqueID();
  std::chrono::steady_clock::time_point requestTime_ =
      std::chrono::steady_clock::now();

  friend bool operator<(
      const HgImportRequest& lhs,
      const HgImportRequest& rhs) {
    return lhs.priority_ < rhs.priority_;
  }
};

} // namespace facebook::eden
