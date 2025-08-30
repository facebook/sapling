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

#include "eden/common/telemetry/RequestMetricsScope.h"
#include "eden/common/utils/Bug.h"
#include "eden/common/utils/IDGen.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/BlobAuxDataFwd.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeAuxDataFwd.h"
#include "eden/fs/store/ImportPriority.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/store/hg/HgProxyHash.h"

namespace facebook::eden {

/**
 * Represents an Sapling import request. This class contains all the necessary
 * information needed to fulfill the request as well as a promise that will be
 * resolved after the requested data is imported. Blobs and Trees also contain
 * a vector of promises to fulfill, corresponding to duplicate requests
 */
class SaplingImportRequest {
 public:
  template <typename ResponseT>
  struct BaseImport {
    using Response = ResponseT;
    BaseImport(ObjectId id, HgProxyHash proxyHash)
        : id{std::move(id)}, proxyHash{std::move(proxyHash)} {}

    ObjectId id;
    HgProxyHash proxyHash;

    // In the case where requests de-duplicate to this one, the requests
    // promise will be enqueued to the following vector.
    std::vector<folly::Promise<Response>> promises;
  };

  using BlobImport = BaseImport<BlobPtr>;
  using TreeImport = BaseImport<TreePtr>;
  using BlobAuxImport = BaseImport<BlobAuxDataPtr>;
  using TreeAuxImport = BaseImport<TreeAuxDataPtr>;

  /**
   * Allocate a blob request.
   */
  static std::shared_ptr<SaplingImportRequest> makeBlobImportRequest(
      const ObjectId& id,
      const HgProxyHash& proxyHash,
      const ObjectFetchContextPtr& context);

  /**
   * Allocate a tree request.
   */
  static std::shared_ptr<SaplingImportRequest> makeTreeImportRequest(
      const ObjectId& id,
      const HgProxyHash& proxyHash,
      const ObjectFetchContextPtr& context);

  static std::shared_ptr<SaplingImportRequest> makeBlobAuxImportRequest(
      const ObjectId& id,
      const HgProxyHash& proxyHash,
      const ObjectFetchContextPtr& context);

  static std::shared_ptr<SaplingImportRequest> makeTreeAuxImportRequest(
      const ObjectId& id,
      const HgProxyHash& proxyHash,
      const ObjectFetchContextPtr& context);

  /**
   * Implementation detail of the make*Request functions from above. Do not
   * use directly.
   */
  template <typename RequestType>
  SaplingImportRequest(
      RequestType request,
      const ObjectFetchContextPtr& context,
      folly::Promise<typename RequestType::Response>&& promise);

  ~SaplingImportRequest() = default;

  SaplingImportRequest(SaplingImportRequest&&) = default;
  SaplingImportRequest& operator=(SaplingImportRequest&&) = default;

  /**
   * Some of the get functions are getting calls from both fetch and
   * prefetch functions. For example, SaplingBackingStore::getBlobImpl()
   * is getting called from both SaplingBackingStore::getBlob() and
   * SaplingBackingStore::prefetchBlob(). This enum will use in the
   * functions params and requests, therefore we can collect correct metrics.
   */
  enum FetchType {
    Fetch,
    Prefetch,
  };

  template <typename T>
  T* getRequest() noexcept {
    return std::get_if<T>(&request_);
  }

  template <typename T>
  bool isType() const noexcept {
    return std::holds_alternative<T>(request_);
  }

  ObjectFetchContextPtr getContext() {
    return context_.copy();
  }

  size_t getType() const noexcept {
    return request_.index();
  }

  ImportPriority getPriority() const noexcept {
    return priority_;
  }

  FetchType getFetchType() const noexcept {
    return fetchType_;
  }

  void setFetchType(FetchType fetch_type) noexcept {
    fetchType_ = fetch_type;
  }

  ObjectFetchContext::Cause getCause() const noexcept {
    return context_->getCause();
  }

  OptionalProcessId getPid() const noexcept {
    return context_->getClientPid();
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
  static std::shared_ptr<SaplingImportRequest> makeRequest(
      const ObjectFetchContextPtr& context,
      Input&&... input);

  SaplingImportRequest(const SaplingImportRequest&) = delete;
  SaplingImportRequest& operator=(const SaplingImportRequest&) = delete;

  using Request =
      std::variant<BlobImport, TreeImport, BlobAuxImport, TreeAuxImport>;
  using Response = std::variant<
      folly::Promise<BlobPtr>,
      folly::Promise<TreePtr>,
      folly::Promise<BlobAuxDataPtr>,
      folly::Promise<TreeAuxDataPtr>>;

  Request request_;
  ObjectFetchContextPtr context_;
  /**
   * Priority of the request. The priority_ get initialized with context's
   * priority. However, down the line we can update the priority of a request,
   * and we don't update priority of the context. For example, before enququing
   * a new request, we check the queue and if this request is already in the
   * queue, we don't enqueue the request again, instead of that we update the
   * priority of the existing request in the queue.
   */
  ImportPriority priority_;
  Response promise_;
  uint64_t unique_ = generateUniqueID();
  // What is the fetch type of this request
  FetchType fetchType_ = FetchType::Fetch;
  std::chrono::steady_clock::time_point requestTime_ =
      std::chrono::steady_clock::now();

  friend bool operator<(
      const SaplingImportRequest& lhs,
      const SaplingImportRequest& rhs) {
    return lhs.priority_ < rhs.priority_;
  }
};

} // namespace facebook::eden
